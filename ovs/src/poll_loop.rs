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
use std::os::raw;
use std::ptr::null;

pub fn fd_wait(fd: raw::c_int, events: raw::c_short) {
    unsafe { sys::poll_fd_wait_at(fd, events, null()) }
}
pub fn timer_wait(msec: i64) {
    unsafe { sys::poll_timer_wait_at(msec, null()) }
}
pub fn timer_wait_until(msec: i64) {
    unsafe { sys::poll_timer_wait_until_at(msec, null()) }
}
pub fn immediate_wake() {
    unsafe { sys::poll_immediate_wake_at(null()) }
}
pub fn block() {
    unsafe { sys::poll_block() }
}

