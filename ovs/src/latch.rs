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
use std::ptr::null;

/// A `Latch` is an Open vSwitch implementation of a thread-safe, signal-safe doorbell that can be
/// polled with `select` and `poll` system calls.  This makes it useful for interfacing with OVS
/// because the OVS libraries otherwise fully encapsulate the OS entities that they work this.  For
/// example, an OVS [`Rconn`] has a file descriptor in it, but the `Rconn` library doesn't provide
/// any way to get the file descriptor out, only a way to call `poll` with it through the OVS
/// `poll_loop`, so it's not possible to make it directly work with anything (like Rust futures)
/// that can't use `poll_loop`.  But Rust futures, etc., can wake up an OVS `poll_loop` using a
/// `Latch`.
pub struct Latch(sys::latch);

impl Latch {
    pub fn new() -> Self {
        let mut latch = Latch(sys::latch { fds: [0, 0] });
        unsafe { sys::latch_init(&mut latch.0) }
        latch
    }
    pub fn poll(&mut self) -> bool { unsafe { sys::latch_poll(&mut self.0) } }
    pub fn set(&mut self) { unsafe { sys::latch_set(&mut self.0) } }
    pub fn is_set(&self) -> bool { unsafe { sys::latch_is_set(&self.0) } }
    pub fn wait(&self) { unsafe { sys::latch_wait_at(&self.0, null()) } }
}

impl Drop for Latch {
    fn drop(&mut self) {
        unsafe { sys::latch_destroy(&mut self.0) }
    }
}
