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

extern crate ddlog_ovsdb_adapter;
extern crate libc;
extern crate ovsdb_sys;
extern crate snvs_ddlog;

use differential_datalog::api::HDDlog;

use differential_datalog::ddval::DDValue;
use differential_datalog::DDlog;
use differential_datalog::DDlogDynamic;
use differential_datalog::DeltaMap;
use differential_datalog::program::Update;

/* Aliases for types in the ovsdb-sys bindings. */
type UpdateEvent = ovsdb_sys::ovsdb_cs_event__bindgen_ty_1_ovsdb_cs_update_event;

#[derive(PartialEq)]
enum ConnectionState {
    /* Initial state before output-only data has been requested. */
    Initial,
    /* Output-only data requested. Waiting for reply. */
    OutputOnlyDataRequested,
    /* Output-only data received. Any request now would be to update data. */
    Update,
}


// TODO: Fill out Context with necessary fields.
pub struct Context {
    prog: HDDlog,
    // ddlog_prog ddlog
    // ddlog_delta *delta /* Accumulated delta to send to OVSDB. */

    /* Database info.
     *
     * The '*_relations' vectors contain DDlog relation names.
     * 'prefix' is the prefix for the DDlog module containing relations. */
    
    prefix: String,
    // input_relations: Vec<String>,
    // output_relations: Vec<String>,
    //output_only_relations: Vec<String>,

    /* OVSDB connection. */
    // cs: ovsdb_sys::ovsdb_cs,
    cs: Option<ovsdb_sys::ovsdb_cs>,
    request_id: Option<ovsdb_sys::json>, /* JSON request ID for outstanding transaction, if any. */
    state: Option<ConnectionState>,

    /* Database info. */
    // db_name: String,
    output_only_data: Option<ovsdb_sys::json>,
    // lock_name: Option<String>, /* Optional name of lock needed. */
    // paused: bool,
}

impl Context {
    /* Process a batch of messages from the database server on 'ctx'. */
    pub unsafe fn run(&mut self) {
        let cs : *mut ovsdb_sys::ovsdb_cs = std::ptr::null_mut();
        let events : *mut ovsdb_sys::ovs_list = std::ptr::null_mut();

        println!("This should cause a segfault");
        ovsdb_sys::ovsdb_cs_run(cs, events);

        let mut updates = Vec::<ovsdb_sys::ovsdb_cs_event>::new();

        // TODO: Confirm the list pointer is advanced correctly.
        while !ovs_list_is_empty(events) {
            let elem = ovs_list_pop_front(events);
            let event = ovs_list_to_event(elem);

            match event.type_ {
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT => {
                    /* 'json_destroy' checks for a null pointer. */
                    ovsdb_sys::json_destroy(self.get_request_id_mut_ptr());
                    self.state = Some(ConnectionState::Initial);
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_LOCKED => {
                    /* Nothing to do here. */
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE => {
                    if event.__bindgen_anon_1.update.clear {
                        updates = Vec::new();
                    }

                    updates.push(event);
                    continue;
                },
                ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_TXN_REPLY => {
                    self.process_txn_reply(event.__bindgen_anon_1.txn_reply);
                },
                _ => {
                    println!("received invalid event type from ovsdb");
                    continue;
                }
            }

            /* TODO: Check if this free is required.
             *
             * Since the event is created within the loop on the Rust side,
             * I don't think we need to free it. Keeping the TODO because I am not sure.
             
            ovsdb_sys::ovsdb_cs_event_destroy(event); */
        }

        self.parse_updates(updates);

        /* 'ovsdb_cs_may_send_transaction' does not check for null.
         * If the optional client-sync is None, early return. */
        let cs_ptr = self.get_cs_mut_ptr();
        if cs_ptr.is_null() {
            return;
        }

        if self.state == Some(ConnectionState::Initial)
        && ovsdb_sys::ovsdb_cs_may_send_transaction(cs_ptr) {
            self.send_output_only_data_request();
        }
    }

    pub unsafe fn process_txn_reply(
        &mut self,
        reply: *mut ovsdb_sys::jsonrpc_msg,
    ) {
        if reply.is_null() {
            println!("received a null transaction reply message");
            return
        }

        /* 'json_equal' checks for a null pointer. */
        let request_id_ptr = self.get_request_id_mut_ptr();
        if !ovsdb_sys::json_equal((*reply).id, request_id_ptr) {
            println!("unexpected transaction reply");
            return;
        }

        /* 'json_destroy' checks for a null pointer. */
        ovsdb_sys::json_destroy(request_id_ptr);
        self.request_id = None;

        if (*reply).type_ == ovsdb_sys::jsonrpc_msg_type_JSONRPC_ERROR {
            let reply_str = ovsdb_sys::jsonrpc_msg_to_string(reply);
            println!("received database error: {:#?}", reply_str);
            // TODO: We need to free the external pointer. libc::free(reply_str);

            /* 'ovsdb_cs_force_reconnect' does not check for a null pointer. */
            let mut cs_ptr = self.get_cs_mut_ptr();
            if cs_ptr.is_null() {
                panic!("needs non-nil client sync to reconnect after txn reply error");
            }

            ovsdb_sys::ovsdb_cs_force_reconnect(cs_ptr);
            return
        }

        match self.state {
            Some(ConnectionState::Initial) => {
                panic!("found initial state while processing transaction reply");
            },
            Some(ConnectionState::OutputOnlyDataRequested) => {
                /* 'json_destroy' checks for a null pointer. */
                ovsdb_sys::json_destroy(self.get_output_only_data_mut_ptr());

                let result_json_ptr = ovsdb_sys::json_clone((*reply).result);
                if result_json_ptr.is_null() {
                    self.output_only_data = None;
                } else {
                    self.output_only_data = Some(*result_json_ptr);
                }

                self.state = Some(ConnectionState::Update);
            },
            Some(ConnectionState::Update) => {
                /* Nothing to do. */
            },
            None => {
                panic!("found invalid state while processing transaction reply");
            }
        }
    }

    pub unsafe fn parse_updates(&mut self, updates_v: Vec<ovsdb_sys::ovsdb_cs_event>) -> Result<(), String> {
        if updates_v.len() == 0 {
            return Ok(());
        }

        self.prog.transaction_start()?;

        for update in updates_v {
            if update.type_ != ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE {
                continue;
            }

            let update_event = update.__bindgen_anon_1.update;
            if update_event.clear /* && TODO !self.ddlog_cleared() */ {
                self.prog.transaction_rollback()?;
            }

            let updates_cp = ovsdb_sys::json_to_string(update_event.table_updates, 0);
            let updates_s: &str = ""; // TODO: Convert updates_cp to &str.

            // Convert prefix into a *const c_char.
            // TODO: There must be an easier way to do this.
            // let prefix_cs = ffi::CString::new(self.prefix.as_str()).unwrap();
            // let prefix_cp = prefix_cs.as_ptr() as *const raw::c_char;

            // ovsdb_api::apply_updates(self.prog, prefix_cp, updates_cp);

            let commands = ddlog_ovsdb_adapter::cmds_from_table_updates_str(self.prefix.as_str(), updates_s)?;
            let updates: Result<Vec<Update<DDValue>>, String> = commands
                .iter()
                .map(|c| self.prog.convert_update_command(c))
                .collect();

            match self.prog.apply_updates(&mut updates?.into_iter()) {
                Ok(_) => {},
                Err(e) => {
                    self.prog.transaction_rollback()?;
                }
            }

            // TODO: free(updates_cp);
        }

        /* Commit changes to DDlog. */
        match self.prog.transaction_commit() {
            Ok(_) => {},
            Err(e) => {
                self.prog.transaction_rollback()?;
            }
        }

        // TODO: Poll immediate wake.

        Ok(())
    }

    pub unsafe fn send_output_only_data_request(&mut self) {
        // TODO: Implement.
    }

    // TODO: Streamline pointer getters.
    // It may be cleaner to unify these getter methods using a general Option<T> function.

    pub fn get_cs_mut_ptr(&mut self) -> *mut ovsdb_sys::ovsdb_cs {
        match self.cs {
            None => std::ptr::null_mut(),
            Some(mut cs) => {
                &mut cs as *mut ovsdb_sys::ovsdb_cs
            }
        }
    }

    pub fn get_request_id_mut_ptr(&mut self) -> *mut ovsdb_sys::json {
        match self.request_id {
            None => std::ptr::null_mut(),
            Some(mut ri) => {
                &mut ri as *mut ovsdb_sys::json
            }
        }
    }

    pub fn get_output_only_data_mut_ptr(&mut self) -> *mut ovsdb_sys::json {
        match self.output_only_data {
            None => std::ptr::null_mut(),
            Some(mut ood) => {
                &mut ood as *mut ovsdb_sys::json
            }
        }
    }
}


// TODO: Loop over this function.
pub fn export_input_from_ovsdb() -> Option<DeltaMap<DDValue>> {
    // Ideally, the handle to the running program would be passed from the controller. Creating a new one here is suboptimal.
    let (prog, init_state) = match snvs_ddlog::run(1, false).ok() {
        Some((p, is)) => (p, is),
        None => return None,
    };

    // TODO: Write proper initializer function.
    let mut ctx = Context {
        prog: prog,
        cs: None,
        request_id: None,
        prefix: String::new(), // Properly initialize.
        state: None,
        output_only_data: None,
    };
    
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
    list: *mut ovsdb_sys::ovs_list,
) -> bool {
    (*list).next == list
}