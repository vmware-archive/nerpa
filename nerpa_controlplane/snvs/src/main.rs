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

use bmv2_packet::*;
use hwaddr::HwAddr;
use nanomsg::{Protocol, Socket};
use packet::Builder;
use std::collections::HashSet;
use std::env;

fn test_packet(dst: HwAddr, src: HwAddr) -> packet::Result<Frame> {
    Ok(Frame(packet::ether::Builder::default().destination(dst)?.source(src)?
             .ip()?.v4()?.destination([1, 2, 3, 4].into())?
             .udp()?.source(1234)?.destination(2345)?
             .build()?))
}

fn main() {
    if env::args().len() != 2 {
        eprintln!("test-snvs, for testing the snvs controller.\n\
usage: {} ENDPOINT\n\
where ENDPOINT is the same nanomsg endpoint passed to bmv2 on --packet-in,\n\
e.g. \"ipc://bmv2.ipc\" for a Unix domain socket in the current directory.",
                  env::args().nth(0).unwrap());
        std::process::exit(1);
    }

    let mut s = Socket::new(Protocol::Pair).unwrap();
    s.connect(&env::args().nth(1).unwrap()).unwrap();

    let e0: HwAddr = [0x00, 0x11, 0x11, 0x00, 0x00, 0x00].into();
    let e1: HwAddr = [0x00, 0x22, 0x22, 0x00, 0x00, 0x00].into();
    let p0 = test_packet(e0, e1).unwrap();
    let p1 = test_packet(e1, e0).unwrap();
    
    s.set_receive_timeout(1000).unwrap();

    // Send 'p0' on port 0 and it should be received on ports 1, 2, and 3.
    // Do it twice: the second time should have the same effect.
    for _i in 0..=1 {
        let replies: HashSet<Bmv2Message> = send_and_receive(&mut s, Bmv2Message::PacketIn { port: 0, payload: p0.clone() }).into_iter().collect();
        assert_eq!(replies, vec![Bmv2Message::PacketOut { port: 1, payload: p0.clone() },
                                 Bmv2Message::PacketOut { port: 2, payload: p0.clone() },
                                 Bmv2Message::PacketOut { port: 3, payload: p0.clone() }].into_iter().collect());
    }

    // Send 'p1' on port 1 with destination MAC as the Ethernet
    // address we just learned on port 0.  It should be received just
    // on port 0.  Again, we might as well do it twice.
    for _i in 0..=1 {
        let replies: HashSet<Bmv2Message> = send_and_receive(&mut s, Bmv2Message::PacketIn { port: 1, payload: p1.clone() }).into_iter().collect();
        assert_eq!(replies, vec![Bmv2Message::PacketOut { port: 0, payload: p1.clone() }].into_iter().collect());
    }

    // Send 'p0' on port 0 again.  This time, it should be received
    // only on port 1 because the destination MAC was learned in the
    // previous step.
    for _i in 0..=1 {
        let replies: HashSet<Bmv2Message> = send_and_receive(&mut s, Bmv2Message::PacketIn { port: 0, payload: p0.clone() }).into_iter().collect();
        assert_eq!(replies, vec![Bmv2Message::PacketOut { port: 1, payload: p0.clone() }].into_iter().collect());
    }

    println!("Success!");
}
