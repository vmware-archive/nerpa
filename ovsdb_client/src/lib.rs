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

#[macro_use]
extern crate memoffset;

#[allow(dead_code)]
mod nerpa_rels;
mod ovs_list;

use serde_json::Value;

use std::{
    ffi,
    os::raw,
};

use differential_datalog::api::HDDlog;

use differential_datalog::ddval::DDValue;
use differential_datalog::DDlog;
use differential_datalog::DDlogDynamic;
use differential_datalog::DeltaMap;
use differential_datalog::program::Update;

/* Aliases for types in the ovsdb-sys bindings. */
type EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type;
const EVENT_TYPE_RECONNECT: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT;
const EVENT_TYPE_LOCKED: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_LOCKED;
const EVENT_TYPE_UPDATE: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE;
const EVENT_TYPE_TXN_REPLY: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_TXN_REPLY;

#[allow(dead_code)]
#[derive(PartialEq)]
enum ConnectionState {
    /* Initial state before output-only data has been requested. */
    Initial,
    /* Output-only data requested. Waiting for reply. */
    OutputOnlyDataRequested,
    /* Output-only data received. Any request now would be to update data. */
    Update,
}

#[repr(C)]
pub struct Context {
    prog: HDDlog,
    pub delta: DeltaMap<DDValue>, /* Accumulated delta to send to OVSDB. */

    /* Database info.
     *
     * The '*_relations' vectors contain DDlog relation names.
     * 'prefix' is the prefix for the DDlog module containing relations. */
    
    prefix: String,
    input_relations: Vec<String>,

    /* OVSDB connection. */
    // TODO: Add client-sync on struct.
    state: Option<ConnectionState>,

    /* Database info. */
    db_name: String,
}

impl Context {
    fn process_txn_reply(
        &mut self,
        cs: *mut ovsdb_sys::ovsdb_cs,
        reply: *mut ovsdb_sys::jsonrpc_msg,
    ) -> Result<(), String> {
        if reply.is_null() {
            return Err(
                format!("received a null transaction reply message")
            );
        }

        /* Dereferencing 'reply' is safe due to the nil check. */
        let reply_type = unsafe{
            (*reply).type_
        };

        if reply_type == ovsdb_sys::jsonrpc_msg_type_JSONRPC_ERROR {
            /* Convert the jsonrpc_msg to a *mut c_char.
             * Represent it in a Rust string for debugging, and free the C string. */
            let reply_s = unsafe {
                let reply_cs = ovsdb_sys::jsonrpc_msg_to_string(reply);
                let reply_s = format!("received database error: {:#?}", reply_cs);
                libc::free(reply_cs as *mut libc::c_void);

                reply_s
            };

            println!("{}", reply_s);

            /* 'ovsdb_cs_force_reconnect' does not check for a null pointer. */
            if cs.is_null() {
                let e = "needs non-nil client sync to force reconnect after txn reply error"; 
                return Err(e.to_string());
            }

            unsafe {
                ovsdb_sys::ovsdb_cs_force_reconnect(cs);
            }

            return Err(reply_s);
        }

        match self.state {
            Some(ConnectionState::Initial) => {
                return Err(
                    format!("found initial state while processing transaction reply")
                );
            },
            Some(ConnectionState::OutputOnlyDataRequested) => {
                // TODO: Store and update 'output_only_data' on Context.

                self.state = Some(ConnectionState::Update);
            },
            Some(ConnectionState::Update) => {}, /* Nothing to do. */
            None => {
                return Err(
                    format!("found invalid state while processing transaction reply")
                );
            }
        }

        Ok(())
    }

    fn parse_updates(
        &mut self,
        updates_v: Vec<ovsdb_sys::ovsdb_cs_event>,
    ) -> Result<(), String> {
        if updates_v.is_empty() {
            return Ok(());
        }

        self.prog.transaction_start()?;

        for update in updates_v {
            if update.type_ != EVENT_TYPE_UPDATE {
                continue;
            }

            let updates_s = unsafe {
                let event = update.__bindgen_anon_1.update;
                let buf = ovsdb_sys::json_to_string(event.table_updates, 0);
                
                ffi::CStr::from_ptr(buf).to_str().unwrap()
            };

            println!("\n\nProcessing update from OVSDB, with message: {}", updates_s);

            
            let commands = ddlog_ovsdb_adapter::cmds_from_table_updates_str(&self.prefix, updates_s)?;

            let updates: Result<Vec<Update<DDValue>>, String> = commands
                .iter()
                .map(|c| self.prog.convert_update_command(c))
                .collect();

            self.prog
                .apply_updates(&mut updates?.into_iter())
                .unwrap_or_else(|e| {
                    self.prog.transaction_rollback().ok();
                    let err = format!("apply_updates failed with error {}", e);
                    println!("{}", err);
                }
            );
            
            // TODO: Determine whether to free updates_s.
        }

        /* Commit changes to DDlog. */
        self.ddlog_commit().unwrap_or_else(|e| {
            self.prog.transaction_rollback().ok();
            let err = format!("transaction_commit failed with error {}", e);
            println!("{}", err);
        });

        // TODO: When this is a long-running program, poll wake.

        Ok(())
    }

    fn ddlog_commit(&mut self) -> Result<(), String> {
        /* We currently overwrite self.delta with the committed result.
         * This works because we loop until we get a result, and then return it.
         * It likely will not work with a more complex management plane. */
        self.delta = self.prog.transaction_commit_dump_changes()?;

        Ok(())
    }
}

unsafe extern "C" fn compose_monitor_request(
    schema_json: *const ovsdb_sys::json,
    _aux: *mut raw::c_void,
) -> *mut ovsdb_sys::json {
    let monitor_requests = ovsdb_sys::json_object_create();

    /* Convert the bindgen-generated 'json' to a Rust 'str'. */
    let schema_cs = ovsdb_sys::json_to_string(schema_json, 0);
    let schema_s = ffi::CStr::from_ptr(schema_cs).to_str().unwrap();

    let json_v: Value = serde_json::from_str(schema_s).unwrap();
    let tables = &json_v["tables"].as_object().unwrap();

    for (tk, tv) in tables.iter() {
        let to = &tv.as_object().unwrap();
        let cols = to["columns"].as_object().unwrap();

        /* Construct a JSON array of each column. */
        let subscribed_cols = ovsdb_sys::json_array_create_empty();
        for (ck, _cv) in cols.iter() {
            let ck_cs = ffi::CString::new(ck.as_str()).unwrap();
            let ck_cp = ck_cs.as_ptr() as *const raw::c_char;

            ovsdb_sys::json_array_add(
                subscribed_cols,
                ovsdb_sys::json_string_create(ck_cp),
            );
        }

        /* Map "columns": [<subscribed_cols>]. */
        let monitor_request = ovsdb_sys::json_object_create();
        let columns_cs = ffi::CString::new("columns").unwrap();
        ovsdb_sys::json_object_put(
            monitor_request,
            columns_cs.as_ptr(),
            subscribed_cols,
        );

        let table_cs = ffi::CString::new(tk.as_str()).unwrap();
        ovsdb_sys::json_object_put(
            monitor_requests,
            table_cs.as_ptr(),
            ovsdb_sys::json_array_create_1(monitor_request),
        );
    }

    /* Log the monitor request. */
    let monitor_requests_cs = ovsdb_sys::json_to_string(monitor_requests, 0);
    let monitor_requests_s = ffi::CStr::from_ptr(monitor_requests_cs).to_str().unwrap();
    println!("\nMonitoring the following OVSDB columns: {}\n", monitor_requests_s);

    monitor_requests
}

pub fn export_input_from_ovsdb(
    server: String,
    database: String,
) -> Option<DeltaMap<DDValue>> {
    let (prog, delta) = match snvs_ddlog::run(1, false).ok() {
        Some((p, is)) => (p, is),
        None => {
            println!("DDlog instance could not be created");
            return None;
        },
    };

    let server_cs = ffi::CString::new(server.as_str()).unwrap();
    let database_cs = ffi::CString::new(database.as_str()).unwrap();

    let prefix = {
        let db = database.clone();
        let lower_prefix = format!("{}_mp::", db);
        
        let mut c = lower_prefix.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().chain(c).collect(),
        }
    };

    let mut ctx = Context {
        prog,
        delta,
        prefix,
        input_relations: nerpa_rels::nerpa_input_relations(),
        state: Some(ConnectionState::Initial),
        db_name: database,
    };

    // We construct the client-sync here so that `ctx` can be passed when creating the connection.
    let cs_ops = &ovsdb_sys::ovsdb_cs_ops {
        compose_monitor_requests: Some(compose_monitor_request),
    } as *const ovsdb_sys::ovsdb_cs_ops;
    
    let cs_ops_void = &mut ctx as *mut Context as *mut ffi::c_void;

    let cs = unsafe {
        let cs = ovsdb_sys::ovsdb_cs_create(
            database_cs.as_ptr(),
            1,
            cs_ops,
            cs_ops_void,
        );
        ovsdb_sys::ovsdb_cs_set_remote(cs, server_cs.as_ptr(), true);
        ovsdb_sys::ovsdb_cs_set_lock(cs, std::ptr::null());

        cs
    };

    loop {
        let mut updates = Vec::<ovsdb_sys::ovsdb_cs_event>::new();

        let mut events = &mut ovs_list::OvsList::default().as_ovs_list();
        unsafe{ovsdb_sys::ovsdb_cs_run(cs, events)};
        while unsafe{!ovs_list::is_empty(events)} {

            /* Advance the pointer, and convert the list to an event. */
            events = unsafe{ovs_list::remove(events).as_mut().unwrap()};
            let event = match unsafe{ovs_list::to_event(events)} {
                None => break,
                Some(e) => e,
            };

            match event.type_ {
                EVENT_TYPE_RECONNECT => {
                    ctx.state = Some(ConnectionState::Initial);
                },
                EVENT_TYPE_LOCKED => {
                    /* Nothing to do here. */
                },
                EVENT_TYPE_UPDATE => {
                    if unsafe{event.__bindgen_anon_1.update.clear} {
                        updates = Vec::new();
                    }

                    updates.push(event);
                    continue;
                },
                EVENT_TYPE_TXN_REPLY => unsafe{ctx.process_txn_reply(cs, event.__bindgen_anon_1.txn_reply).ok()?},
                _ => {
                    println!("received invalid event type from ovsdb");
                    continue;
                }
            }

            break;

            /* TODO: Check if this free is required.
             *
             * Since the event is created within the loop on the Rust side,
             * we may not need to free it. Keeping the TODO because I am not sure.
             
            ovsdb_sys::ovsdb_cs_event_destroy(event); */
        }
        println!("Received {} update events from OVSDB.", updates.len());
        ctx.parse_updates(updates).ok()?;

        if ctx.delta.len() > 0 {
            return Some(ctx.delta);
        }

        std::thread::sleep(std::time::Duration::from_millis(10 * 1000));
    }
}
