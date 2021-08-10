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

mod hmap;

mod nerpa_rels;

#[allow(dead_code)]
mod ovs_list;

use serde_json::Value;

use std::convert::TryFrom;
use std::{
    ffi,
    os::raw,
    ptr,
};

use differential_datalog::api::HDDlog;

use differential_datalog::ddval::DDValue;
use differential_datalog::DDlog;
use differential_datalog::DDlogDynamic;
use differential_datalog::DeltaMap;
use differential_datalog::program::{RelId, Update};
use differential_datalog::record::IntoRecord;

use snvs_ddlog::Relations;
use snvs_ddlog::ovsdb_api;

/* Aliases for types in the ovsdb-sys bindings. */
type EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type;
const EVENT_TYPE_RECONNECT: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_RECONNECT;
const EVENT_TYPE_LOCKED: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_LOCKED;
const EVENT_TYPE_UPDATE: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_UPDATE;
const EVENT_TYPE_TXN_REPLY: EventType = ovsdb_sys::ovsdb_cs_event_ovsdb_cs_event_type_OVSDB_CS_EVENT_TYPE_TXN_REPLY;

#[derive(PartialEq)]
enum ConnectionState {
    /* Initial state before output-only data has been requested. */
    Initial,
    /* Output-only data requested. Waiting for reply. */
    OutputOnlyDataRequested,
    /* Output-only data received. Any request now would be to update data. */
    Update,
}

pub struct Context {
    prog: HDDlog,
    pub delta: DeltaMap<DDValue>, /* Accumulated delta to send to OVSDB. */

    /* Database info.
     *
     * The '*_relations' vectors contain DDlog relation names.
     * 'prefix' is the prefix for the DDlog module containing relations. */
    
    prefix: String,
    input_relations: Vec<String>,
    output_relations: Vec<String>,
    output_only_relations: Vec<String>,

    /* OVSDB connection. */
    cs: Option<ovsdb_sys::ovsdb_cs>,
    request_id: Option<ovsdb_sys::json>, /* JSON request ID for outstanding transaction, if any. */
    state: Option<ConnectionState>,

    /* Database info. */
    db_name: String,
    output_only_data: Option<ovsdb_sys::json>,

    /* TODO: As the management plane usage becomes more complex, these fields may become useful.
    lock_name: Option<String>, /* Optional name of lock needed. */
    paused: bool, */
}

impl Context {
    /// Process a batch of messages from the database server.
    /// # Safety
    /// Context.cs must be non-None.
    pub unsafe fn run(&mut self) -> Result<(), String> {
        if self.cs.is_none() {
            let e = "must establish client-sync before processing messages";
            return Err(e.to_string());
        }

        let cs = self.get_cs_mut_ptr();
        if cs.is_null() {
            let e = "got null pointer from client-sync";
            return Err(e.to_string())
        }

        let mut events = &mut ovs_list::OvsList::default().to_ovs_list();
        ovsdb_sys::ovsdb_cs_run(cs, events);

        let mut updates = Vec::<ovsdb_sys::ovsdb_cs_event>::new();

        // TODO: Confirm the list pointer is advanced correctly.
        while !ovs_list::is_empty(events) {
            /* Extract the event from the intrusive list received from OVSDB. */
            let opt_event = ovs_list::to_event(events);
            let event = match opt_event {
                None => break,
                Some(e) => e,
            };

            /* `events` should be non-null, since `to_event` checks null.
             * This dereferences events; advances the pointer; and assigns a mutable reference to events. */
            events = &mut *((*events).next);

            match event.type_ {
                EVENT_TYPE_RECONNECT => {
                    /* TODO: Check if needed: 'json_destroy'. */
                    self.request_id = None;
                    self.state = Some(ConnectionState::Initial);
                },
                EVENT_TYPE_LOCKED => {
                    /* Nothing to do here. */
                },
                EVENT_TYPE_UPDATE => {
                    if event.__bindgen_anon_1.update.clear {
                        updates = Vec::new();
                    }

                    updates.push(event);
                    continue;
                },
                EVENT_TYPE_TXN_REPLY => {
                    self.process_txn_reply(event.__bindgen_anon_1.txn_reply)?;
                },
                _ => {
                    println!("received invalid event type from ovsdb");
                    continue;
                }
            }

            /* TODO: Check if this free is required.
             *
             * Since the event is created within the loop on the Rust side,
             * we may not need to free it. Keeping the TODO because I am not sure.
             
            ovsdb_sys::ovsdb_cs_event_destroy(event); */
        }
        self.parse_updates(updates)?;

        /* 'ovsdb_cs_may_send_transaction' does not check for null.
         * If the optional client-sync is None, early return. */
        let cs_ptr = self.get_cs_mut_ptr();
        if cs_ptr.is_null() {
            let e = "found empty client-sync after parsing updates";
            return Err(e.to_string());
        }

        if self.state == Some(ConnectionState::Initial)
        && ovsdb_sys::ovsdb_cs_may_send_transaction(cs_ptr) {
            self.send_output_only_data_request()?;
        }

        Ok(())
    }

    pub unsafe fn process_txn_reply(
        &mut self,
        reply: *mut ovsdb_sys::jsonrpc_msg,
    ) -> Result<(), String> {
        if reply.is_null() {
            let e = "received a null transaction reply message";
            return Err(e.to_string());
        }

        /* 'json_equal' checks for null pointers. */
        let request_id_ptr = self.get_request_id_mut_ptr();
        if !ovsdb_sys::json_equal((*reply).id, request_id_ptr) {
            let e = "transaction reply has incorrect request id";
            return Err(e.to_string());
        }

        /* 'json_destroy' checks for a null pointer. */
        ovsdb_sys::json_destroy(request_id_ptr);
        self.request_id = None;

        if (*reply).type_ == ovsdb_sys::jsonrpc_msg_type_JSONRPC_ERROR {
            // Convert the jsonrpc_msg to a *mut c_char.
            // Represent it in a Rust string for debugging, and free the C string.
            let reply_cs = ovsdb_sys::jsonrpc_msg_to_string(reply);
            let reply_e = format!("received database error: {:#?}", reply_cs);
            println!("{}", reply_e);
            libc::free(reply_cs as *mut libc::c_void);

            /* 'ovsdb_cs_force_reconnect' does not check for a null pointer. */
            let cs_ptr = self.get_cs_mut_ptr();
            if cs_ptr.is_null() {
                let e = "needs non-nil client sync to force reconnect after txn reply error"; 
                return Err(e.to_string());
            }

            ovsdb_sys::ovsdb_cs_force_reconnect(cs_ptr);
            return Err(reply_e.to_string());
        }

        match self.state {
            Some(ConnectionState::Initial) => {
                let e = "found initial state while processing transaction reply";
                return Err(e.to_string());
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
                let e = "found invalid state while processing transaction reply";
                return Err(e.to_string());
            }
        }

        Ok(())
    }

    unsafe fn parse_updates(
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

            let update_event = update.__bindgen_anon_1.update;

            /* TODO: Put back in once we process the updates successfully.
            if update_event.clear && self.ddlog_cleared() {
                self.prog.transaction_rollback()?;
                return Ok(());
            } */

            let ddlog_ptr = &self.prog as *const HDDlog;

            let updates_buf: *const raw::c_char = ovsdb_sys::json_to_string(update_event.table_updates, 0);
            let updates_s: &str = ffi::CStr::from_ptr(updates_buf).to_str().unwrap();
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

        // TODO: Poll immediate wake. This will be needed when this is a long-running program.

        Ok(())
    }

    fn ddlog_commit(&mut self) -> Result<(), String> {
        /* We currently overwrite self.delta with the committed result.
         * This works because we loop until we get a result, and then return it.
         * It likely will not work with a more complex management plane. */
        
        // TODO: Check if we need to remove warnings from deltas.

        let new_delta = self.prog.transaction_commit_dump_changes()?;
        self.delta = new_delta;

        Ok(())
    }

    fn ddlog_cleared(&mut self) -> bool {
        let mut num_failures = 0;
        for input_relation in self.input_relations.iter() {
            let table = format!("{}{}", self.prefix, input_relation);
            let tid = match self.prog.inventory.get_table_id(table.as_str()) {
                Ok(relid) => relid,
                Err(_) => std::usize::MAX as RelId,
            };

            num_failures += self.prog.clear_relation(tid).map(|_| 0).unwrap_or_else(|_|{1});
        }

        num_failures == 0
    }

    /* Sends the database server a request for all row UUIDs in output-only tables. */
    unsafe fn send_output_only_data_request(&mut self) -> Result<(), String> {
        if !self.output_only_relations.is_empty() {
            // TODO: Check if needed: json_destroy(ctx->output_only_data)
            self.output_only_data = None;

            let db_s = ffi::CString::new(self.db_name.as_str()).unwrap();
            let ops = ovsdb_sys::json_array_create_1(
                ovsdb_sys::json_string_create(db_s.as_ptr()));
            
            for output_only_rel in self.output_only_relations.iter() {
                let op = ovsdb_sys::json_object_create();
                
                let op_s = ffi::CString::new("op").unwrap();
                let select_s = ffi::CString::new("select").unwrap();
                ovsdb_sys::json_object_put_string(
                    op,
                    op_s.as_ptr(),
                    select_s.as_ptr(),
                );

                let table_s = ffi::CString::new("table").unwrap();
                let oor_s = ffi::CString::new(output_only_rel.as_str()).unwrap();
                ovsdb_sys::json_object_put_string(
                    op,
                    table_s.as_ptr(),
                    oor_s.as_ptr(),
                );

                let columns_s = ffi::CString::new("columns").unwrap();
                let uuid_s = ffi::CString::new("_uuid").unwrap();
                let uuid_json = ovsdb_sys::json_string_create(uuid_s.as_ptr());
                ovsdb_sys::json_object_put(
                    op,
                    columns_s.as_ptr(),
                    ovsdb_sys::json_array_create_1(uuid_json),
                );

                let where_s = ffi::CString::new("where").unwrap();
                ovsdb_sys::json_object_put(
                    op,
                    where_s.as_ptr(),
                    ovsdb_sys::json_array_create_empty(),
                );

                ovsdb_sys::json_array_add(ops, op);
            }

            self.state = Some(ConnectionState::OutputOnlyDataRequested);

            // Set the context request_id using the OVSDB response.
            if self.cs.is_none() {
                let e = "found empty client-sync when sending output-only data request";
                return Err(e.to_string());
            }

            let tx_request_id = ovsdb_sys::ovsdb_cs_send_transaction(self.get_cs_mut_ptr(), ops);
            if tx_request_id.is_null() {
                self.request_id = None;
            } else {
                self.request_id = Some(*tx_request_id);
            }
        } else {
            self.state = Some(ConnectionState::Update);   
        }

        Ok(())
    }

    // TODO: Streamline pointer getters.
    // It may be cleaner to unify these getter methods using a general Option<T> function.

    pub fn get_cs_mut_ptr(&mut self) -> *mut ovsdb_sys::ovsdb_cs {
        match self.cs {
            None => ptr::null_mut(),
            Some(mut cs) => {
                &mut cs as *mut ovsdb_sys::ovsdb_cs
            }
        }
    }

    pub fn get_request_id_mut_ptr(&mut self) -> *mut ovsdb_sys::json {
        match self.request_id {
            None => ptr::null_mut(),
            Some(mut ri) => {
                &mut ri as *mut ovsdb_sys::json
            }
        }
    }

    pub fn get_output_only_data_mut_ptr(&mut self) -> *mut ovsdb_sys::json {
        match self.output_only_data {
            None => ptr::null_mut(),
            Some(mut ood) => {
                &mut ood as *mut ovsdb_sys::json
            }
        }
    }
}

unsafe extern "C" fn compose_monitor_request(
    schema_json: *const ovsdb_sys::json,
    aux: *mut raw::c_void,
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
        for (ck, cv) in cols.iter() {
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

    // Print monitor_requests out for debugging.
    let monitor_requests_cs: *const raw::c_char = ovsdb_sys::json_to_string(monitor_requests, 0);
    let monitor_requests_s: &str = ffi::CStr::from_ptr(monitor_requests_cs).to_str().unwrap();
    println!("\nMonitoring the following OVSDB columns: {}\n", monitor_requests_s);

    monitor_requests
}

// Temporary function until pointer provenance issues with the expected workflow are fixed.
pub unsafe fn create_context_and_loop(
    server: String,
    database: String,
    input_relations: Vec<String>,
    output_relations: Vec<String>,
    output_only_relations: Vec<String>,
) -> Option<DeltaMap<DDValue>> {
    let (prog, delta) = match snvs_ddlog::run(1, false).ok() {
        Some((p, is)) => (p, is),
        None => {
            println!("DDlog instance could not be created");
            return None;
        },
    };
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
        prog: prog,
        delta: delta,
        prefix: prefix,
        input_relations: input_relations,
        output_relations: output_relations,
        output_only_relations: output_only_relations,
        cs: None, /* We set this below, so we can pass `ctx` as a pointer. */
        request_id: None, /* This gets set later. */
        state: Some(ConnectionState::Initial),
        output_only_data: None, /* This will get filled in later. */
        db_name: database,
    };

    // We construct the client-sync here so that `ctx` can be passed when creating the connection.
    let cs_ops = ovsdb_sys::ovsdb_cs_ops {
        compose_monitor_requests: Some(compose_monitor_request),
    };
    
    let cs_ops_void = &mut ctx as *mut Context as *mut ffi::c_void;

    let cs = ovsdb_sys::ovsdb_cs_create(
        database_cs.as_ptr(),
        1,
        &cs_ops as *const ovsdb_sys::ovsdb_cs_ops,
        cs_ops_void,
    );

    let server_cs = ffi::CString::new(server.as_str()).unwrap();
    ovsdb_sys::ovsdb_cs_set_remote(cs, server_cs.as_ptr(), true);
    ovsdb_sys::ovsdb_cs_set_lock(cs, std::ptr::null());

    loop {
        // this loops over the logic of `run()` without the previous pointer provenance issues.

        let mut events = &mut ovs_list::OvsList::default().to_ovs_list();
        ovsdb_sys::ovsdb_cs_run(cs, events);
        let mut updates = Vec::<ovsdb_sys::ovsdb_cs_event>::new();
        while !ovs_list::is_empty(events) {
            /* Advance the pointer, and convert the list to an event. */
            events = ovs_list::remove(events).as_mut().unwrap();
            let event = match ovs_list::to_event(events) {
                None => {
                    break;
                },
                Some(e) => {
                    e
                }
            };

            match event.type_ {
                EVENT_TYPE_RECONNECT => {
                    /* TODO: Check if needed: 'json_destroy'. */
                    ctx.request_id = None;
                    ctx.state = Some(ConnectionState::Initial);
                },
                EVENT_TYPE_LOCKED => {
                    /* Nothing to do here. */
                },
                EVENT_TYPE_UPDATE => {
                    if event.__bindgen_anon_1.update.clear {
                        updates = Vec::new();
                    }

                    updates.push(event);
                    continue;
                },
                EVENT_TYPE_TXN_REPLY => {
                    ctx.process_txn_reply(event.__bindgen_anon_1.txn_reply).ok()?
                },
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

        if ctx.state == Some(ConnectionState::Initial)
        && ovsdb_sys::ovsdb_cs_may_send_transaction(cs) {
            ctx.send_output_only_data_request().ok()?;
        }


        if ctx.delta.len() > 0 {
            return Some(ctx.delta);
        }

        std::thread::sleep(std::time::Duration::from_millis(10 * 1000));
    }

    None
}

pub fn create_context(
    server: String,
    database: String,
    input_relations: Vec<String>,
    output_relations: Vec<String>,
    output_only_relations: Vec<String>,
) -> Option<Context> {
    // TODO: Ideally, the handle to the running program would be passed from the controller. Creating a new one here is suboptimal.
    let (prog, delta) = match snvs_ddlog::run(1, false).ok() {
        Some((p, is)) => (p, is),
        None => {
            println!("DDlog instance could not be created");
            return None;
        },
    };
    let database_cs = ffi::CString::new(database.as_str()).unwrap();

    let mut ctx = Context {
        prog: prog,
        delta: delta,
        prefix: format!("{}::", database),
        input_relations: input_relations,
        output_relations: output_relations,
        output_only_relations: output_only_relations,
        cs: None, /* We set this below, so we can pass `ctx` as a pointer. */
        request_id: None, /* This gets set later. */
        state: Some(ConnectionState::Initial),
        output_only_data: None, /* This will get filled in later. */
        db_name: database,
    };

    // We construct the client-sync here so that `ctx` can be passed when creating the connection.
    ctx.cs = unsafe {
        let cs_ops = ovsdb_sys::ovsdb_cs_ops {
            compose_monitor_requests: Some(compose_monitor_request),
        };
        
        let cs_ops_void = &mut ctx as *mut Context as *mut ffi::c_void;

        let cs = ovsdb_sys::ovsdb_cs_create(
            database_cs.as_ptr(),
            1,
            &cs_ops as *const ovsdb_sys::ovsdb_cs_ops,
            cs_ops_void,
        );

        let server_cs = ffi::CString::new(server.as_str()).unwrap();
        ovsdb_sys::ovsdb_cs_set_remote(cs, server_cs.as_ptr(), true);
        ovsdb_sys::ovsdb_cs_set_lock(cs, std::ptr::null());

        match cs.is_null() {
            true => None,
            false => Some(*cs),
        }
    };

    Some(ctx)
}

pub fn export_input_from_ovsdb(
    server: String,
    database: String,
) -> Option<DeltaMap<DDValue>> {
    let (prog, delta) = match snvs_ddlog::run(1, false).ok() {
        Some((p, is)) => (p, is),
        None => return None,
    };

    unsafe{create_context_and_loop(
        server,
        database,
        nerpa_rels::nerpa_input_relations(),
        nerpa_rels::nerpa_output_relations(),
        nerpa_rels::nerpa_output_only_relations(),
    )}
}
