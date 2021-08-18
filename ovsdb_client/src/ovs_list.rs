/*
Copyright (c) 2021 VMware, Inc.
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

/* OVS list functions. Because these are `inline`, bindgen does not generate them. */

extern crate ovsdb_sys;

use std::{
    cell::Cell as Mut,
    ptr,
};

// Should have same C representation as `ovs_list` from bindgen crate.
#[repr(C)]
#[derive(Debug, Default)]
pub struct OvsList {
    pub prev: Mut<Option<ptr::NonNull<OvsList>>>,
    pub next: Mut<Option<ptr::NonNull<OvsList>>>,
}

impl OvsList {
    pub fn as_ovs_list(&mut self) -> ovsdb_sys::ovs_list {
        let prev_ptr = match self.prev.get() {
            None => ptr::null_mut(),
            Some(p) => p.as_ptr() as *mut ovsdb_sys::ovs_list,
        };

        let next_ptr = match self.next.get() {
            None => ptr::null_mut(),
            Some(p) => p.as_ptr() as *mut ovsdb_sys::ovs_list,
        };

        ovsdb_sys::ovs_list {
            prev: prev_ptr,
            next: next_ptr,
        }
    }
}

/* Cast an ovs_list to an ovsdb_cs_event. */
pub unsafe fn to_event(
    list_ptr: *mut ovsdb_sys::ovs_list
) -> Option<ovsdb_sys::ovsdb_cs_event> {
    if list_ptr.is_null() {
        return None;
    }

    let event_ptr = list_ptr
        .cast::<u8>()
        .wrapping_sub(offset_of!(ovsdb_sys::ovsdb_cs_event, list_node))
        .cast::<ovsdb_sys::ovsdb_cs_event>();
    
    if event_ptr.is_null() {
        return None;
    }

    Some(*event_ptr)
}

pub unsafe fn remove(
    elem: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    (*(*elem).prev).next = (*elem).next;
    (*(*elem).next).prev = (*elem).prev;
    
    (*elem).next
}

pub unsafe fn is_empty(
    list: *mut ovsdb_sys::ovs_list,
) -> bool {
    (*list).next == list
}
