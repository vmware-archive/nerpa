/*!
Packet interface to bmv2, for testing.

Usually, the [P4 behavioral
model](https://github.com/p4lang/behavioral-model) (aka bmv2) uses
host network devices for packet input and output.  For testing, this
means creating virtual network interfaces such as veth devices.  That
works, but in turn it requires host operating system support and,
usually, superuser privileges, which can be inconvenient, especially
for CI/CD.

As an alternative, bmv2 also supports simulating network devices.
This library provides an interface to bmv2's
[nanomsg](https://nanomsg.org/)-based support for simulating devices.
(There's another one based on grpc instead.  I do not see a clear
advantage to either one.  They do about the same thing.)

To configure bmv2 to use nanomsg-simulated network devices, pass
it the `--packet-in` option.  (Pass `-s` to the `run-nerpa.sh`
script to make it do this.)
*/
#![warn(missing_docs)]
/*
Copyright (c) 2022 VMware, Inc.
SPDX-License-Identifier: MIT
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

use anyhow::Result;
use nanomsg::Socket;
use packet::Packet;
use std::convert::{TryFrom, TryInto};
use std::io::{Read, Write, ErrorKind};
use thiserror::Error;

/// These must match the values in
/// <https://github.com/p4lang/behavioral-model/blob/main/src/bm_sim/dev_mgr_packet_in.cpp>.
const MSG_TYPE_PORT_ADD: i32 = 0;
const MSG_TYPE_PORT_REMOVE: i32 = 1;
const MSG_TYPE_PORT_SET_STATUS: i32 = 2;
const MSG_TYPE_PACKET_IN: i32 = 3;
const MSG_TYPE_PACKET_OUT: i32 = 4;
const MSG_TYPE_INFO_REQ: i32 = 5;
const MSG_TYPE_INFO_REP: i32 = 6;

/// These must match the values for `PortStatus`
/// in <https://github.com/p4lang/behavioral-model/blob/main/include/bm/bm_sim/port_monitor.h>.
mod port_status {
    pub const PORT_UP: i32 = 2;
    pub const PORT_DOWN: i32 = 3;
}

/// An Ethernet frame.
///
/// This is a "newtype" style struct so we can define `Debug` on it.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Frame(pub Vec<u8>);

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let eth = match packet::ether::Packet::new(&self.0) {
            Ok(packet) => packet,
            Err(e) => return write!(f, "{}", e)
        };
        write!(f, "eth(dst={}, src={}), ", eth.destination(), eth.source())?;

        match eth.protocol() {
            packet::ether::Protocol::Ipv4 => {
                let ipv4 = match packet::ip::v4::Packet::new(eth.payload()) {
                    Ok(packet) => packet,
                    Err(e) => return write!(f, "bad_ipv4({})", e)
                };
                write!(f, "ipv4(dst={}, src={}), ", ipv4.destination(), ipv4.source())?;

                match ipv4.protocol() {
                    packet::ip::Protocol::Udp => {
                        let udp = match packet::udp::Packet::new(ipv4.payload()) {
                            Ok(packet) => packet,
                            Err(e) => return write!(f, "bad_udp({:?})", e)
                        };
                        write!(f, "udp(dst={}, src={})", udp.destination(), udp.source())?;
                    },
                    packet::ip::Protocol::Tcp => {
                        let tcp = match packet::tcp::Packet::new(ipv4.payload()) {
                            Ok(packet) => packet,
                            Err(e) => return write!(f, "bad_tcp({:?})", e)
                        };
                        write!(f, "tcp(dst={}, src={})", tcp.destination(), tcp.source())?;
                    },
                    packet::ip::Protocol::Icmp => {
                        let icmp = match packet::icmp::Packet::new(ipv4.payload()) {
                            Ok(packet) => packet,
                            Err(e) => return write!(f, "bad_icmp({:?})", e)
                        };
                        write!(f, "icmp(type={:?}, code={})", icmp.kind(), icmp.code())?;
                    },
                    protocol => return write!(f, "ipproto({:?})", protocol)
                };
            },
            packet::ether::Protocol::Ipv6 => {
                match packet::ip::v6::Packet::new(eth.payload()) {
                    Ok(_) => return write!(f, "ipv6()"),
                    Err(e) => return write!(f, "bad_ipv6({})", e)
                };
            },
            protocol => return write!(f, "ethertype({:?})", protocol)
        };
        Ok(())
    }
}

/// A message that can be sent or received on the nanomsg connection to
/// bmv2 when the `--packet-in` option is used.
#[derive(Eq, Hash, PartialEq)]
pub enum Bmv2Message {
    /// The client sends this message to bmv2 to create a port with
    /// the given number.  The port is initially up.  If a port with
    /// this number already exists, nothing happens.
    PortAdd(i32),

    /// The client sends this message to bmv2 to the port with the
    /// given number from the switch.  If no port with this number
    /// exists, nothing happens.
    PortRemove(i32),

    /// The client sends this message to bmv2 to bring the given port
    /// up.  If no port `port` exists, nothing happens.
    PortUp(i32),

    /// The client sends this message to bmv2 to take the given port
    /// down.  If no port `port` exists, nothing happens.
    PortDown(i32),

    /// The client sends this message to bmv2 to cause a packet to be
    /// received and processed by the switch program.
    PacketIn {
        /// Port to receive the packet.
        ///
        /// If no such port exists, or if it is down, nothing happens.
        port: i32,

        /// Packet to be received.
        payload: Frame
    },

    /// bmv2 sends this message to reports that the switch program
    /// sent a packet to a port.
    PacketOut {
        /// Port to which the program sent the packet.
        port: i32,

        /// Packet that was sent.
        payload: Frame
    },

    /// The client may send this to obtain an InfoRep reply.  Perhaps
    /// this is useful as an application-level "echo" protocol.
    InfoReq,

    /// bmv2 sends this message in response to an InfoRep message.
    InfoRep,
}

impl std::fmt::Debug for Bmv2Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bmv2Message::PortAdd(port) => write!(f, "PortAdd({})", port),
            Bmv2Message::PortRemove(port) => write!(f, "PortRemove({})", port),
            Bmv2Message::PortUp(port) => write!(f, "PortUp({})", port),
            Bmv2Message::PortDown(port) => write!(f, "PortDown({})", port),
            Bmv2Message::PacketIn { port, payload } => write!(f, "PacketIn({}, {:?})", port, payload),
            Bmv2Message::PacketOut { port, payload } => write!(f, "PacketOut({}, {:?})", port, payload),
            Bmv2Message::InfoReq => write!(f, "InfoReq"),
            Bmv2Message::InfoRep => write!(f, "InfoRep"),
        }
    }
}

/// Error that can arise converting `Vec<u8>` to `Bmv2Message`.
#[derive(Error, Debug)]
pub enum Bmv2MessageError {
    /// Message is too short.
    #[error("message too short ({0} bytes)")]
    BadLength(usize),

    /// Unknown message type.
    #[error("unknown message type {0}")]
    UnknownType(i32)
}

impl TryFrom<Vec<u8>> for Bmv2Message {
    type Error = anyhow::Error;
    fn try_from(msg: Vec<u8>) -> Result<Self> {
        if msg.len() < 12 {
            return Err(Bmv2MessageError::BadLength(msg.len()).into())
        }
        let type_ = i32::from_ne_bytes(msg[0..4].try_into()?);
        let port = i32::from_ne_bytes(msg[4..8].try_into()?);
        let more = i32::from_ne_bytes(msg[8..12].try_into()?);
        let payload = Frame(msg[12..].to_vec());
        Ok(match type_ {
            MSG_TYPE_PORT_ADD => Bmv2Message::PortAdd(port),
            MSG_TYPE_PORT_REMOVE => Bmv2Message::PortRemove(port),
            MSG_TYPE_PORT_SET_STATUS => if more == port_status::PORT_UP {
                Bmv2Message::PortUp(port)
            } else {
                Bmv2Message::PortDown(port)
            },
            MSG_TYPE_PACKET_IN => Bmv2Message::PacketIn { port, payload },
            MSG_TYPE_PACKET_OUT => Bmv2Message::PacketOut { port, payload },
            MSG_TYPE_INFO_REQ => Bmv2Message::InfoReq,
            MSG_TYPE_INFO_REP => Bmv2Message::InfoRep,
            _ => return Err(Bmv2MessageError::UnknownType(type_).into())
        })
    }
}

impl From<Bmv2Message> for Vec<u8> {
    fn from(msg: Bmv2Message) -> Self {
        let (type_, port, more, payload) = match msg {
            Bmv2Message::PortAdd(port) => (MSG_TYPE_PORT_ADD, port, 0, vec![]),
            Bmv2Message::PortRemove(port) => (MSG_TYPE_PORT_REMOVE, port, 0, vec![]),
            Bmv2Message::PortUp(port) => (MSG_TYPE_PORT_SET_STATUS, port, port_status::PORT_UP, vec![]),
            Bmv2Message::PortDown(port) => (MSG_TYPE_PORT_SET_STATUS, port, port_status::PORT_DOWN, vec![]),
            Bmv2Message::PacketIn { port, payload } => (MSG_TYPE_PACKET_IN, port, payload.0.len() as i32, payload.0),
            Bmv2Message::PacketOut { port, payload } => (MSG_TYPE_PACKET_OUT, port, payload.0.len() as i32, payload.0),
            Bmv2Message::InfoReq => (MSG_TYPE_INFO_REQ, 0, 0, vec![]),
            Bmv2Message::InfoRep => (MSG_TYPE_INFO_REP, 0, 0, vec![]),
        };
        i32::to_ne_bytes(type_).iter()
            .chain(i32::to_ne_bytes(port).iter())
            .chain(i32::to_ne_bytes(more).iter())
            .chain(payload.iter())
            .cloned()
            .collect()
    }
}

/// Sends `request` on `s`, then waits for replies until no more replies have been received for one
/// second, and returns the replies.
///
/// Ordinarily, `s` should be a `Bmv2Message::PacketOut` to cause a packet to be received on a port.
///
/// Prints the requests and replies on stdout.
///
/// Panics on I/O error.
pub fn send_and_receive(s: &mut Socket, request: Bmv2Message) -> Vec<Bmv2Message> {
    println!("send {:?}", request);
    s.write_all(&Vec::<u8>::from(request)).unwrap();

    s.set_receive_timeout(1000).unwrap();
    let mut replies = Vec::new();
    loop {
        let mut msg = Vec::new();
        match s.read_to_end(&mut msg) {
            Ok(_) => (),
            Err(ref e) if e.kind() == ErrorKind::TimedOut => break,
            Err(err) => panic!("read_to_end(): {}", err)
        };
        let reply = Bmv2Message::try_from(msg).unwrap();
        println!("receive {:?}", reply);
        replies.push(reply);
    };
    println!();
    replies
}
