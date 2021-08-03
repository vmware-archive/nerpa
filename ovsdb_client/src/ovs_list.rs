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

/* Aliases for types in the ovsdb-sys bindings. */
type UpdateEvent = ovsdb_sys::ovsdb_cs_event__bindgen_ty_1_ovsdb_cs_update_event;

// Should have same C representation as `ovs_list` from bindgen crate.
#[repr(C)]
#[derive(Debug, Default)]
pub struct OvsList {
    pub prev: Mut<Option<ptr::NonNull<OvsList>>>,
    pub next: Mut<Option<ptr::NonNull<OvsList>>>,
}

impl OvsList {
    pub fn to_ovs_list(&mut self) -> ovsdb_sys::ovs_list {
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

// Should have same C representation as `ovsdb_cs_event` from bindgen crate.
#[repr(C)]
pub struct OvsdbCsEvent {
    pub list_node: OvsList,
    pub type_: ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type,
    pub __bindgen_anon_1: ovsdb_sys::ovsdb_cs_event__bindgen_ty_1,
}


/* Cast an ovs_list to an ovsdb_cs_event. */
pub unsafe fn to_event(
    list: *mut ovsdb_sys::ovs_list
) -> ovsdb_sys::ovsdb_cs_event {
    // Translate the node to the intrusive list.
    // TODO: Implement this from the OVS macros, rather than hardcoding.
    let update_event = UpdateEvent {
        clear: true,
        monitor_reply: true,
        table_updates: std::ptr::null_mut(),
        version: 0,
    };

    ovsdb_sys::ovsdb_cs_event {
        list_node: *list,
        type_: ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT,
        __bindgen_anon_1: ovsdb_sys::ovsdb_cs_event__bindgen_ty_1 {
            update: update_event,
        }
    }
}


/* Initializes 'list' as an empty list. */
// TODO: Rewrite this function in a Rust-like manner.
pub unsafe fn init(list: *mut ovsdb_sys::ovs_list) {
    (*list).prev = list;
    (*list).next = list;
}

/* Initializes 'list' with pointers that cause segfaults if dereferenced and will show up in a debugger. */
pub unsafe fn poison(list: *mut ovsdb_sys::ovs_list) {
    *list = ovsdb_sys::OVS_LIST_POISON;
}

// TODO: Implement `splice`.

/* Insert 'elem' just before 'before'. */
pub unsafe fn insert(
    before: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    (*elem).prev = (*before).prev;
    (*elem).next = before;
    (*(*before).prev).next = elem;
    (*before).prev = elem;
}

/* Insert 'elem' at the beginning of 'list', so it becomes the front in 'list'. */
pub unsafe fn push_front(
    list: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    insert((*list).next, elem);
}

/* Insert 'elem' at the end of 'list', so it becomes the back in 'list'. */
pub unsafe fn push_back(
    list: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    insert(list, elem);
}

/* Puts 'elem' in the position currently occupied by 'position'.
 * Afterward, 'position' is not part of a list. */
pub unsafe fn replace(
    element: *mut ovsdb_sys::ovs_list,
    position: *const ovsdb_sys::ovs_list,
) {
    (*element).next = (*position).next;
    (*(*element).next).prev = element;

    (*element).prev = (*position).prev;
    (*(*element).prev).next = element;
}

// TODO: Implement `moved`.

pub unsafe fn remove(
    elem: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    (*(*elem).prev).next = (*elem).next;
    (*(*elem).next).prev = (*elem).prev;
    
    (*elem).next
}

pub unsafe fn pop_front(
    list: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    let front: *mut ovsdb_sys::ovs_list = (*list).next;

    remove(front);
    front
}

pub unsafe fn pop_back(
    list: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    let back: *mut ovsdb_sys::ovs_list = (*list).prev;

    remove(back);
    back
}

pub unsafe fn is_empty(
    list: *mut ovsdb_sys::ovs_list,
) -> bool {
    (*list).next == list
}
