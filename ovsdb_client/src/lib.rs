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

extern crate ovsdb_sys;

use differential_datalog::api::HDDlog;

use differential_datalog::ddval::DDValue;
use differential_datalog::DeltaMap;

/* Aliases for OVS event structs for readability. */


// TODO: Fill out Context with necessary fields.
pub struct Context {}

impl Context {
    /* Process a batch of messages from the database server on 'ctx'. */
    pub unsafe fn run(&mut self) {
        let cs : *mut ovsdb_sys::ovsdb_cs = std::ptr::null_mut();
        let events : *mut ovsdb_sys::ovs_list = std::ptr::null_mut();

        println!("This should cause a segfault");
        ovsdb_sys::ovsdb_cs_run(cs, events);

        // TODO: Confirm the list pointer is advanced correctly.
        while !ovs_list_is_empty(events) {
            let elem = ovs_list_pop_front(events);
            let event = ovs_list_to_event(elem);

            match event.type_ {
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT => {
                    /*
                    TODO: Destroy the ctx-> request_id JSON.
                    TODO: Set the ctx->state as initial.
                    */
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_LOCKED => {
                    /* Nothing to do here. */
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE => {
                    // TODO: Add event to list of updates.
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_TXN_REPLY => {
                    // TODO: Process the transaction reply.
                },
            }

            // TODO: Confirm that this free is even necessary.
            ovsdb_sys::ovsdb_cs_event_destroy(event);
        }

        // TODO: Parse update list.
        // TODO: If necessary, destroy the event list.

        // TODO: If state is initial and the ovsdb client can send the transaction, send an output only data request.
    }
}


// TODO: Loop over this function.
pub fn export_input_from_ovsdb(
    mut ddlog: &HDDlog
) -> Option<DeltaMap<DDValue>> {
    let mut ctx = Context{};
    
    unsafe {
        ctx.run();
    }

    None
}

unsafe fn ovs_list_to_event(
    list: *mut ovsdb_sys::ovs_list
) -> ovsdb_sys::ovsdb_cs_event {
    // Translate the node to the intrusive list.
    // TODO: Implement this from the OVS macros, rather than hardcoding.
    let update_event = ovsdb_cs_event__bindgen_ty_1_ovsdb_cs_update_event {
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
            txn_reply: std::ptr::null_mut(),
        }
    }
}

/* OVS list functions. Because these are `inline`, bindgen does not generate them. */
// TODO: Move these to a separate module.

/* Initializes 'list' as an empty list. */
// TODO: Rewrite this function in a Rust-like manner.
unsafe fn ovs_list_init(list: *mut ovsdb_sys::ovs_list) {
    (*list).prev = list;
    (*list).next = list;
}

/* Initializes 'list' with pointers that cause segfaults if dereferenced and will show up in a debugger. */
unsafe fn ovs_list_poison(list: *mut ovsdb_sys::ovs_list) {
    *list = ovsdb_sys::OVS_LIST_POISON;
}

// TODO: Implement `ovs_list_splice`.

/* Insert 'elem' just before 'before'. */
unsafe fn ovs_list_insert(
    before: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    (*elem).prev = (*before).prev;
    (*elem).next = before;
    (*(*before).prev).next = elem;
    (*before).prev = elem;
}

/* Insert 'elem' at the beginning of 'list', so it becomes the front in 'list'. */
unsafe fn ovs_list_push_front(
    list: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    ovs_list_insert((*list).next, elem);
}

/* Insert 'elem' at the end of 'list', so it becomes the back in 'list'. */
unsafe fn ovs_list_push_back(
    list: *mut ovsdb_sys::ovs_list,
    elem: *mut ovsdb_sys::ovs_list,
) {
    ovs_list_insert(list, elem);
}

/* Puts 'elem' in the position currently occupied by 'position'.
 * Afterward, 'position' is not part of a list. */
unsafe fn ovs_list_replace(
    element: *mut ovsdb_sys::ovs_list,
    position: *const ovsdb_sys::ovs_list,
) {
    (*element).next = (*position).next;
    (*(*element).next).prev = element;

    (*element).prev = (*position).prev;
    (*(*element).prev).next = element;
}

// TODO: Implement `ovs_list_moved`.

unsafe fn ovs_list_remove(
    elem: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    (*(*elem).prev).next = (*elem).next;
    (*(*elem).next).prev = (*elem).prev;
    return (*elem).next;
}

unsafe fn ovs_list_pop_front(
    list: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    let front: *mut ovsdb_sys::ovs_list = (*list).next;

    ovs_list_remove(front);
    return front;
}

unsafe fn ovs_list_pop_back(
    list: *mut ovsdb_sys::ovs_list
) -> *mut ovsdb_sys::ovs_list {
    let back: *mut ovsdb_sys::ovs_list = (*list).prev;

    ovs_list_remove(back);
    return back;
}

unsafe fn ovs_list_is_empty(
    list: *const ovsdb_sys::ovs_list,
) -> bool {
    (*list).next == list
}