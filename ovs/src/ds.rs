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

use std::ffi;
use std::ptr::null_mut;

/// Wrapper for OVS `struct ds`, which is a dynamically allocated string type.  Rust code should
/// use `std::string::String` instead.
///
/// `Ds` is useful for other OVS wrappers because it is part of the interface for some OVS
/// functions.
pub struct Ds(pub sys::ds);

impl Ds {
    pub fn new() -> Ds {
        Ds(sys::ds { string: null_mut(), length: 0, allocated: 0 })
    }
}

impl Drop for Ds {
    fn drop(&mut self) {
        unsafe { sys::ds_destroy(&mut self.0 as *mut _); }
    }
}

impl From<Ds> for String {
    fn from(ds: Ds) -> Self {
        unsafe {
            let p = sys::ds_cstr_ro(&ds.0 as *const _);
            ffi::CStr::from_ptr(p).to_string_lossy().into()
        }
    }
}
