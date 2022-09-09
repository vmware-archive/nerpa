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
use super::ofp_errors;

use std::ffi;
use std::fmt;
use std::mem;

use anyhow::Result;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfpRaw(pub sys::ofpraw);

impl OfpRaw {
    pub fn is_valid(self) -> bool {
        // There's no easy way to get the maximum ofpraw value.  This is the biggest one as of this
        // writing.
        return self.0 <= sys::ofpraw_OFPRAW_NXST_IPFIX_FLOW_REPLY;
    }

    pub fn decode(oh: &[u8]) -> Result<Self> {
        if oh.len() < mem::size_of::<sys::ofp_header>() {
            Err(ofp_errors::Error(sys::ofperr::OFPERR_OFPBRC_BAD_LEN))?
        }

        let mut raw: sys::ofpraw = 0;
        unsafe {
            ofp_errors::parse(sys::ofpraw_decode(&mut raw as *mut sys::ofpraw,
                                                 oh.as_ptr() as *const sys::ofp_header))?;
        }
        Ok(OfpRaw(raw))
    }
}

impl fmt::Display for OfpRaw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            if self.is_valid() {
                let s = ffi::CStr::from_ptr(sys::ofpraw_get_name(self.0));
                write!(f, "{}", s.to_string_lossy())
            } else {
                write!(f, "<unknown ofpraw {}>", self.0)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfpType(pub sys::ofptype);

impl OfpType {
    pub fn is_valid(self) -> bool {
        // There's no easy way to get the maximum ofptype value.  This is the biggest one as of
        // this writing.
        return self.0 <= sys::ofptype_OFPTYPE_FLOW_MONITOR_RESUMED;
    }

    pub fn decode(oh: &[u8]) -> Result<Self> {
        if oh.len() < mem::size_of::<sys::ofp_header>() {
            Err(ofp_errors::Error(sys::ofperr::OFPERR_OFPBRC_BAD_LEN))?
        }

        let mut ofptype: sys::ofptype = 0;
        unsafe {
            ofp_errors::parse(sys::ofptype_decode(&mut ofptype as *mut sys::ofptype,
                                                  oh.as_ptr() as *const sys::ofp_header))?;
        }
        Ok(OfpType(ofptype))
    }
}

impl fmt::Display for OfpType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            if self.is_valid() {
                let s = ffi::CStr::from_ptr(sys::ofptype_get_name(self.0));
                write!(f, "{}", s.to_string_lossy())
            } else {
                write!(f, "<unknown ofptype {}>", self.0)
            }
        }
    }
}


pub fn update_length(buf: &mut Ofpbuf) {
    unsafe {
        sys::ofpmsg_update_length(buf.0 as *mut _);
    }
}

