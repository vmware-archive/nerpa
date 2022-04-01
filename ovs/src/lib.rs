/*
Copyright (c) 2021, 2022 VMware, Inc.
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

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

//! `ovs` provides Rust wrappers for Open vSwitch libraries.

/// The `sys` module contains the raw `bindgen` bindings for OVS libraries.
/// It's usually better to use a safe wrapper around these bindings.
pub mod sys {
    use std::ptr::null_mut;
    use std::slice;

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

    impl Default for ofputil_flow_mod {
        fn default() -> Self {
            Self {
                list_node: ovs_list { next: null_mut(), prev: null_mut() },
                match_: minimatch { flow: null_mut(), mask: null_mut(), tun_md: null_mut() },
                priority: 0,
                cookie: 0,
                cookie_mask: 0,
                modify_cookie: false,
                new_cookie: 0,
                table_id: 0,
                command: 0,
                idle_timeout: 0,
                hard_timeout: 0,
                buffer_id: 0,
                out_port: 0,
                out_group: 0,
                flags: 0,
                importance: 0,
                ofpacts: null_mut(),
                ofpacts_len: 0,
                ofpacts_tlv_bitmap: 0
            }
        }
    }

    impl From<ofpbuf> for Vec<u8> {
        fn from(buf: ofpbuf) -> Self {
            unsafe { slice::from_raw_parts(buf.data as *const u8, buf.size as usize).into() }
        }
    }
}

pub mod ds;
pub mod latch;
pub mod ofpbuf;
pub mod ofp_bundle;
pub mod ofp_errors;
pub mod ofp_flow;
pub mod ofp_msgs;
pub mod ofp_print;
pub mod ofp_protocol;
pub mod poll_loop;
pub mod rconn;
