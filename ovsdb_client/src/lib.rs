/*!
Rust wrapper for OVSDB.

Allows Rust programs to interact with a running
[OVSDB](https://datatracker.ietf.org/doc/html/rfc7047.txt)
instance.  
*/
#![warn(missing_docs)]
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
extern crate ovs;

#[macro_use]
extern crate memoffset;

#[allow(dead_code)]
mod ovs_list;
/// Context that interacts with OVSDB.
pub mod context;

use serde_json::Value;

use std::{ffi, os::raw};

use differential_datalog::ddval::DDValue;
use differential_datalog::program::Update;

use tokio::sync::mpsc;

/// Aliases for types in the ovs bindings.
type EventType = ovs::sys::ovsdb_cs_event_ovsdb_cs_event_type;
const EVENT_TYPE_RECONNECT: EventType = ovs::sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT;
const EVENT_TYPE_LOCKED: EventType = ovs::sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_LOCKED;
const EVENT_TYPE_UPDATE: EventType = ovs::sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE;
const EVENT_TYPE_TXN_REPLY: EventType = ovs::sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_TXN_REPLY;

/// Compose request to monitor changes to specific columns in OVSDB.
///
/// Used internally, to construct the OVSDB connection.
///
/// # Arguments
/// * `schema_json` - OVSDB schema.
/// * `_aux` - additional data for the request.
unsafe extern "C" fn compose_monitor_request(
    schema_json: *const ovs::sys::json,
    _aux: *mut raw::c_void,
) -> *mut ovs::sys::json {
    let monitor_requests = ovs::sys::json_object_create();

    // Convert the bindgen-generated 'json' to a Rust 'str'.
    let schema_cs = ovs::sys::json_to_string(schema_json, 0);
    let schema_s = ffi::CStr::from_ptr(schema_cs).to_str().unwrap();

    let json_v: Value = serde_json::from_str(schema_s).unwrap();
    let tables = &json_v["tables"].as_object().unwrap();

    for (tk, tv) in tables.iter() {
        let to = &tv.as_object().unwrap();
        let cols = to["columns"].as_object().unwrap();

        // Construct a JSON array of each column.
        let subscribed_cols = ovs::sys::json_array_create_empty();
        for (ck, _cv) in cols.iter() {
            let ck_cs = ffi::CString::new(ck.as_str()).unwrap();
            let ck_cp = ck_cs.as_ptr() as *const raw::c_char;

            ovs::sys::json_array_add(
                subscribed_cols,
                ovs::sys::json_string_create(ck_cp),
            );
        }

        // Map "columns": [<subscribed_cols>].
        let monitor_request = ovs::sys::json_object_create();
        let columns_cs = ffi::CString::new("columns").unwrap();
        ovs::sys::json_object_put(
            monitor_request,
            columns_cs.as_ptr(),
            subscribed_cols,
        );

        let table_cs = ffi::CString::new(tk.as_str()).unwrap();
        ovs::sys::json_object_put(
            monitor_requests,
            table_cs.as_ptr(),
            ovs::sys::json_array_create_1(monitor_request),
        );
    }

    // Log the monitor request.
    let monitor_requests_cs = ovs::sys::json_to_string(monitor_requests, 0);
    let monitor_requests_s = ffi::CStr::from_ptr(monitor_requests_cs).to_str().unwrap();
    println!("\nMonitoring the following OVSDB columns: {}\n", monitor_requests_s);

    monitor_requests
}

/// A mutable, raw pointer to a live OVSDB connection.
//
// This is a "newtype" style struct, so we can define `Send` on it.
struct OvsdbCSPtr(*mut ovs::sys::ovsdb_cs);

// The `Send` trait lets us transfer an object across thread boundaries.
// This pointer is only used in single-threaded settings, so the trait is unimplemented.
// It exists so that this type can be used in a function called by a Tokio actor.
unsafe impl Send for OvsdbCSPtr{}

/// Process inputs from OVSDB.
///
/// # Arguments
/// * `ctx` - context to communicate with OVSDB.
/// * `server` - filepath to OVSDB server.
/// * `database` - name for OVSDB database.
/// * `respond_to` - sender for DDlog inputs (as updates) to an external program.
pub async fn process_ovsdb_inputs(
    mut ctx: context::OvsdbContext,
    server: String,
    database: String,
    respond_to: mpsc::Sender<Option<Update<DDValue>>>,
) -> Result<(), String> {
    let server_cs = ffi::CString::new(server.as_str()).unwrap();
    let database_cs = ffi::CString::new(database.as_str()).unwrap();

    // Construct the client-sync here, so `ctx` can be passed when creating the connection.
    let cs_ptr = unsafe {
        let cs_ops = &ovs::sys::ovsdb_cs_ops {
            compose_monitor_requests: Some(compose_monitor_request),
        } as *const ovs::sys::ovsdb_cs_ops;
        let cs_ops_void = &mut ctx as *mut context::OvsdbContext as *mut ffi::c_void;

        let cs = ovs::sys::ovsdb_cs_create(
            database_cs.as_ptr(),
            1,
            cs_ops,
            cs_ops_void,
        );
        ovs::sys::ovsdb_cs_set_remote(cs, server_cs.as_ptr(), true);
        ovs::sys::ovsdb_cs_set_lock(cs, std::ptr::null());

        OvsdbCSPtr(cs)
    };

    loop {
        let updates = unsafe {
            let mut event_updates = Vec::<ovs::sys::ovsdb_cs_event>::new();
            let cs = cs_ptr.0;

            let mut events_list = &mut ovs_list::OvsList::default().as_ovs_list();
            ovs::sys::ovsdb_cs_run(cs, events_list);

            while !ovs_list::is_empty(events_list) {
                events_list = ovs_list::remove(events_list).as_mut().unwrap();
                let event = match ovs_list::to_event(events_list) {
                    None => break,
                    Some(e) => e,
                };

                match event.type_ {
                    EVENT_TYPE_RECONNECT => {
                        ctx.state = Some(context::ConnectionState::Initial);
                    },
                    EVENT_TYPE_LOCKED => {
                        /* Nothing to do here. */
                    },
                    EVENT_TYPE_UPDATE => {
                        if event.__bindgen_anon_1.update.clear {
                            event_updates = Vec::new();
                        }

                        event_updates.push(event);
                        continue;
                    },
                    EVENT_TYPE_TXN_REPLY => {
                        let reply_res = ctx.process_txn_reply(cs, event.__bindgen_anon_1.txn_reply);
                        if reply_res.is_err() {
                            println!("could not process txn reply with error: {:#?}", reply_res.err());
                        }
                    },
                    _ => {
                        println!("received invalid event type from ovsdb");
                        continue;
                    }
                }
            }

            ctx.parse_updates(event_updates)
        };

        for update in updates {
            let send_res = respond_to.send(Some(update)).await;
            if send_res.is_err() {
                println!("could not send update from ovsdb client to controller: {:#?}", send_res.err());
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10 * 1000));
    }
}
