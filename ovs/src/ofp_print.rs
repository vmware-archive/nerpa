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
use std::ffi;
use std::fmt;
use std::os::raw;
use std::ptr::null;

pub struct Printer<'a>(pub &'a [u8]);

impl<'a> fmt::Display for Printer<'a> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            let p = sys::ofp_to_string(
                self.0.as_ptr() as *const raw::c_void,
                self.0.len() as sys::size_t, null(), null(), 1);
            let s = ffi::CStr::from_ptr(p);
            write!(formatter, "{}", s.to_string_lossy())?;
            libc::free(p as *mut raw::c_void);
            Ok(())
        }
    }
}
