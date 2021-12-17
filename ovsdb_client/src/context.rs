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

#[allow(dead_code)]
mod nerpa_rels;

use crate::EVENT_TYPE_UPDATE;

use differential_datalog::api::HDDlog;
use differential_datalog::ddval::DDValue;
use differential_datalog::DeltaMap;
use differential_datalog::program::Update;

use std::ffi;

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
    pub state: Option<ConnectionState>,

    /* Database info. */
    db_name: String,
}

#[allow(dead_code)]
#[derive(PartialEq)]
pub enum ConnectionState {
    /* Initial state before output-only data has been requested. */
    Initial,
    /* Output-only data requested. Waiting for reply. */
    OutputOnlyDataRequested,
    /* Output-only data received. Any request now would be to update data. */
    Update,
}

impl Context {
    pub fn new(
        prog: HDDlog,
        delta: DeltaMap<DDValue>,
        name: String,
    ) -> Self {
        let prefix = {
            let db = name.clone();
            let lower_prefix = format!("{}_mp::", db);

            let mut lpc = lower_prefix.chars();
            match lpc.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(lpc).collect(),
            }
        };

        Self {
            prog,
            delta,
            prefix,
            input_relations: nerpa_rels::nerpa_input_relations(),
            state: Some(ConnectionState::Initial),
            db_name: name,
        }
    }

    /// Processes a TXN_REPLY event from OVSDB.
    ///
    /// # Safety
    ///
    /// This function is marked unsafe because it dereferences a possibly null raw pointer.
    /// Because it checks if this pointer is null, its behavior will be safe.
    pub unsafe fn process_txn_reply(
        &mut self,
        cs: *mut ovsdb_sys::ovsdb_cs,
        reply: *mut ovsdb_sys::jsonrpc_msg,
    ) -> Result<(), String> {
        if reply.is_null() {
            return Err(
                "received a null transaction reply message".to_string()
            );
        }

        /* Dereferencing 'reply' is safe due to the nil check. */
        let reply_type = (*reply).type_;

        if reply_type == ovsdb_sys::jsonrpc_msg_type_JSONRPC_ERROR {
            /* Convert the jsonrpc_msg to a *mut c_char.
             * Represent it in a Rust string for debugging, and free the C string. */
            let reply_s = {
                let reply_cs = ovsdb_sys::jsonrpc_msg_to_string(reply);
                let reply_s = format!("received database error: {:#?}", reply_cs);
                libc::free(reply_cs as *mut libc::c_void);

                reply_s
            };

            /* 'ovsdb_cs_force_reconnect' does not check for a null pointer. */
            if cs.is_null() {
                return Err(
                    "needs non-nil client sync to force reconnect after txn reply error".to_string()
                );
            }

            ovsdb_sys::ovsdb_cs_force_reconnect(cs);

            return Err(reply_s);
        }

        match self.state {
            Some(ConnectionState::Initial) => {
                return Err(
                    "found initial state while processing transaction reply".to_string()
                );
            },
            Some(ConnectionState::OutputOnlyDataRequested) => {
                // TODO: Store and update 'output_only_data' on Context.

                self.state = Some(ConnectionState::Update);
            },
            Some(ConnectionState::Update) => {}, /* Nothing to do. */
            None => {
                return Err(
                    "found invalid state while processing transaction reply".to_string()
                );
            }
        }

        Ok(())
    }

    pub fn parse_table_updates(
        &self,
        table_updates:Vec<String>,
    ) -> Vec<Update<DDValue>> {
        let mut updates = Vec::new();
        
        if table_updates.is_empty() {
            return updates;
        }

        for table_update in table_updates {
            let commands_res = ddlog_ovsdb_adapter::cmds_from_table_updates_str(
                &self.prefix,
                &table_update
            );

            if commands_res.is_err() {
                println!("error extracting commands from table updates: {}", commands_res.unwrap_err());
                continue;
            }

            let updates_res: Result<Vec<Update<DDValue>>, String> = commands_res
                .unwrap()
                .iter()
                .map(|c| self.prog.convert_update_command(c))
                .collect();

            match updates_res {
                Err(e) => println!("error converting update command: {}", e),
                Ok(mut r) => updates.append(&mut r),
            };
        }

        updates
    }

    pub fn parse_updates(
        &self,
        events: Vec<ovsdb_sys::ovsdb_cs_event>,
    ) -> Vec<Update<DDValue>> {
        let mut updates = Vec::new();

        if events.is_empty() {
            return updates;
        }

        for event in events {
            if event.type_ != EVENT_TYPE_UPDATE {
                continue;
            }

            let table_updates_s = unsafe {
                let update = event.__bindgen_anon_1.update;
                let buf = ovsdb_sys::json_to_string(update.table_updates, 0);

                ffi::CStr::from_ptr(buf).to_str().unwrap()
            };

            let commands_res = ddlog_ovsdb_adapter::cmds_from_table_updates_str(
                &self.prefix,
                table_updates_s
            );

            if commands_res.is_err() {
                println!("error extracting commands from table updates: {}", commands_res.unwrap_err());
                continue;
            }

            let updates_res: Result<Vec<Update<DDValue>>, String> = commands_res
                .unwrap()
                .iter()
                .map(|c| self.prog.convert_update_command(c))
                .collect();

            match updates_res {
                Err(e) => println!("error converting update command: {}", e),
                Ok(mut r) => updates.append(&mut r),
            };
        }

        updates
    }
}