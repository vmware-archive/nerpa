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
use super::sys;
use super::ofpbuf::Ofpbuf;
use super::ofp_protocol::Versions;

use std::ffi;
use std::io;
use std::ptr::{null, null_mut};

pub struct Rconn(*mut sys::rconn);

pub const DSCP_DEFAULT: u8 = sys::DSCP_DEFAULT as u8;
impl Rconn {
    pub fn new(inactivity_probe_interval: i32, max_backoff: i32,
               dscp: u8, versions: Versions) -> Rconn {
        unsafe {
            Rconn(sys::rconn_create(inactivity_probe_interval, max_backoff, dscp,
                                    versions.bits()))
        }
    }

    pub fn set_dscp(&mut self, dscp: u8) {
        unsafe { sys::rconn_set_dscp(self.0, dscp) }
    }
    pub fn dscp(&self) -> u8 {
        unsafe { sys::rconn_get_dscp(self.0) }
    }

    pub fn set_max_backoff(&mut self, max_backoff: i32) {
        unsafe { sys::rconn_set_max_backoff(self.0, max_backoff) }
    }
    pub fn max_backoff(&self) -> i32 {
        unsafe { sys::rconn_get_max_backoff(self.0) }
    }

    pub fn set_probe_interval(&mut self, inactivity_probe_interval: i32) {
        unsafe { sys::rconn_set_probe_interval(self.0, inactivity_probe_interval) }
    }
    pub fn probe_interval(&self) -> i32 {
        unsafe { sys::rconn_get_probe_interval(self.0) }
    }

    pub fn connect(&mut self, target: &str, name: Option<&str>) {
        unsafe {
            let target = ffi::CString::new(target).unwrap();
            let name = name.map(|s| ffi::CString::new(s).unwrap());
            sys::rconn_connect(self.0, target.as_ptr(), name.map_or(null(), |n| n.as_ptr()))
        }
    }
    //XXX: fn connect_unreliably()
    pub fn reconnect(&mut self) {
        unsafe { sys::rconn_reconnect(self.0) }
    }
    pub fn disconnect(&mut self) {
        unsafe { sys::rconn_disconnect(self.0) }
    }

    pub fn run(&mut self) {
        unsafe { sys::rconn_run(self.0) }
    }
    pub fn run_wait(&mut self) {
        unsafe { sys::rconn_run(self.0) }
    }
    pub fn recv(&mut self) -> Option<Ofpbuf> {
        unsafe {
            let msg = sys::rconn_recv(self.0);
            if msg == null_mut() {
                None
            } else {
                Some(Ofpbuf::from_ptr(msg))
            }
        }
    }
    pub fn recv_wait(&mut self) {
        unsafe { sys::rconn_recv_wait(self.0) }
    }
    // XXX rconn_packet_counter
    pub fn send(&mut self, mut buf: Ofpbuf) -> io::Result<()> {
        unsafe {
            match sys::rconn_send(self.0, buf.leak(), null_mut()) {
                0 => Ok(()),
                errno => Err(io::Error::from_raw_os_error(errno))
            }
        }
    }
    // XXX send_with_limit()

    // XXX add_monitor()

    pub fn name(&self) -> String {
        unsafe { ffi::CStr::from_ptr(sys::rconn_get_name(self.0)).to_string_lossy().into() }
    }
    pub fn set_name(&mut self, name: &str) {
        unsafe {
            let name = ffi::CString::new(name).unwrap();
            sys::rconn_set_name(self.0, name.as_ptr())
        }
    }
    pub fn target(&self) -> String {
        unsafe { ffi::CStr::from_ptr(sys::rconn_get_target(self.0)).to_string_lossy().into() }
    }

    pub fn reliable(&self) -> bool { unsafe { sys::rconn_is_reliable(self.0) } }
    pub fn alive(&self) -> bool { unsafe { sys::rconn_is_alive(self.0) } }
    pub fn connected(&self) -> bool { unsafe { sys::rconn_is_connected(self.0) } }
    pub fn admitted(&self) -> bool { unsafe { sys::rconn_is_admitted(self.0) } }
    pub fn failure_duration(&self) -> i32 { unsafe { sys::rconn_failure_duration(self.0) } }

    pub fn version(&self) -> i32 { unsafe { sys::rconn_get_version(self.0) } }

    pub fn state(&self) -> String {
        unsafe { ffi::CStr::from_ptr(sys::rconn_get_state(self.0)).to_string_lossy().into() }
    }
    pub fn last_connection(&self) -> i64 { unsafe { sys::rconn_get_last_connection(self.0) } }
    pub fn last_disconnect(&self) -> i64 { unsafe { sys::rconn_get_last_disconnect(self.0) } }
    pub fn connection_seqno(&self) -> u32 { unsafe { sys::rconn_get_connection_seqno(self.0) } }
    pub fn last_error(&self) -> i32 { unsafe { sys::rconn_get_last_error(self.0) } }
    pub fn txqlen(&self) -> u32 { unsafe { sys::rconn_count_txqlen(self.0) } }
}

impl Drop for Rconn {
    fn drop(&mut self) {
        unsafe { sys::rconn_destroy(self.0) }
    }
}

