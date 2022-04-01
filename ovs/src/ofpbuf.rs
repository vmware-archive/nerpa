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

use libc;
use std::slice;
use std::ptr::null_mut;

pub struct Ofpbuf(pub *mut sys::ofpbuf);

unsafe impl Send for Ofpbuf {}
unsafe impl Sync for Ofpbuf {}
impl Ofpbuf {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((*self.0).data as *const u8, (*self.0).size as usize) }
    }
    pub fn as_ptr(&self) -> *const u8 {
        unsafe { (*self.0).data as *const u8 }
    }
    pub fn from_ptr(buf: *mut sys::ofpbuf) -> Ofpbuf {
        Ofpbuf(buf)
    }
    pub unsafe fn leak(&mut self) -> *mut sys::ofpbuf {
        let ptr = self.0;
        self.0 = null_mut();
        ptr
    }
}

impl From<Ofpbuf> for Vec<u8> {
    fn from(buf: Ofpbuf) -> Vec<u8> {
        buf.as_slice().into()
    }
}

impl Drop for Ofpbuf {
    fn drop(&mut self) {
        unsafe {
            if self.0 != null_mut() {
                sys::ofpbuf_uninit(self.0);
                libc::free(self.0 as *mut _);
            }
        }
    }
}

