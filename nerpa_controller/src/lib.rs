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

extern crate grpcio;
extern crate proto;
extern crate protobuf;

// The auto-generated crate `snvs_ddlog` declares the `HDDlog` type.
// This serves as a reference to a running DDlog program.
// It implements `trait differential_datalog::DDlog`.
use snvs_ddlog::api::HDDlog;

// `differential_datalog` contains the DDlog runtime copied to each generated workspace.
use differential_datalog::DDlog; // Trait that must be implemented by DDlog program.
use differential_datalog::DDlogDynamic;
use differential_datalog::DeltaMap; // Represents a set of changes to DDlog relations.
use differential_datalog::ddval::DDValue; // Generic type wrapping all DDlog values.
use differential_datalog::program::Update;
use differential_datalog::record::{Record, IntoRecord};

use p4ext::{ActionRef, Table};

use proto::p4runtime_grpc::P4RuntimeClient;

use std::collections::HashMap;

// Controller
// It contains a handle to the DDlog program, so we can use it to determine the form of packets.
pub struct Controller {
    hddlog: HDDlog,
}

impl Controller {
    pub fn new() -> Result<Controller, String> {
        let (hddlog, _init_state) = HDDlog::run(1, false)?;
        
        Ok(Self{hddlog})
    }

    pub fn stop(&mut self) {
        self.hddlog.stop().unwrap();
    }

    pub fn add_input(&mut self, updates: Vec<Update<DDValue>>) -> Result<DeltaMap<DDValue>, String> {
        self.hddlog.transaction_start()?;
        
        let update_result = self.hddlog.apply_updates(&mut updates.into_iter());
        match update_result {
            Ok(_) => {},
            Err(_) => { self.hddlog.transaction_rollback()? }
        };

        self.hddlog.transaction_commit_dump_changes()
    }

    pub fn dump_delta(delta: &DeltaMap<DDValue>) {
        for (rel, changes) in delta.iter() {
            println!("Changes to relation {}", snvs_ddlog::relid2name(*rel).unwrap());
            for (val, weight) in changes.iter() {
                println!("{} {:+}", val, weight);
            }
        }
    }

    pub fn push_outputs_to_switch(
        delta: &DeltaMap<DDValue>,
        device_id: u64,
        role_id: u64,
        target: &str,
        client: &P4RuntimeClient,
    ) -> Result<(), p4ext::P4Error> {
        let mut updates = Vec::new();

        let pipeline = p4ext::get_pipeline_config(device_id, target, client);
        let switch: p4ext::Switch = pipeline.get_p4info().into();

        for (_rel_id, output_map) in (*delta).clone().into_iter() {
            for (value, _weight) in output_map {
                let record = value.clone().into_record();
                
                match record {
                    Record::NamedStruct(name, recs) => {
                        // Translate the record table name to the P4 table name.
                        let mut table: Table = Table::default();
                        let mut table_name: String = "".to_string();

                        match Self::get_matching_table(name.to_string(), switch.tables.clone()) {
                            Some(t) => {
                                table = t;
                                table_name = table.preamble.name;
                            },
                            None => {},
                        };

                        // Iterate through fields in the record.
                        // Map all match keys to values.
                        // If the field is the action, extract the action, name, and parameters.
                        let mut action_name: String = "".to_string();
                        let matches = &mut HashMap::<std::string::String, u16>::new();
                        let params = &mut HashMap::<std::string::String, u16>::new();
                        let mut priority: i32 = 0;
                        for (_, (fname, v)) in recs.iter().enumerate() {
                            let match_name: String = fname.to_string();

                            match match_name.as_str() {
                                "action" => {
                                    match v {
                                        Record::NamedStruct(name, arecs) => {
                                            // Find matching action name from P4 table.
                                            action_name = match Self::get_matching_action_name(name.to_string(), table.actions.clone()) {
                                                Some(an) => an,
                                                None => "".to_string()
                                            };
    
                                            // Extract param values from action's records.
                                            for (_, (afname, aval)) in arecs.iter().enumerate() {
                                                params.insert(afname.to_string(), Self::extract_record_value(&aval));
                                            }
                                        },
                                        _ => println!("action was incorrectly formed!")
                                    }
                                },
                                "priority" => {
                                    priority = Self::extract_record_value(&v).into();
                                },
                                _ => {
                                    matches.insert(match_name, Self::extract_record_value(&v));
                                }
                            }
                        }

                        // If we found a table and action, construct a P4 table entry update.
                        if !(table_name.is_empty() || action_name.is_empty()) {
                            let update = p4ext::build_table_entry_update(
                                proto::p4runtime::Update_Type::INSERT,
                                table_name.as_str(),
                                action_name.as_str(),
                                params,
                                matches,
                                priority,
                                device_id,
                                target,
                                client,
                            ).unwrap_or_else(|err| panic!("could not build table update: {}", err));
                            updates.push(update);
                        }
                    }
                    _ => {
                        println!("record was not named struct");
                    }
                }
            }
        }

        p4ext::write(updates, device_id, role_id, target, client)
    }

    fn extract_record_value(r: &Record) -> u16 {
        use num_traits::cast::ToPrimitive;
        match r {
            Record::Bool(true) => 1,
            Record::Bool(false) => 0,
            Record::Int(i) => i.to_u16().unwrap_or(0),
            // TODO: Handle other types.
            _ => 1,
        }
    }

    fn get_matching_table(record_name: String, tables: Vec<Table>) -> Option<Table> {
        for t in tables {
            let tn = &t.preamble.name;
            let tv: Vec<String> = tn.split('.').map(|s| s.to_string()).collect();
            let ts = tv.last().unwrap();

            if record_name.contains(ts) {
                return Some(t);
            }
        }

        None
    }

    fn get_matching_action_name(record_name: String, actions: Vec<ActionRef>) -> Option<String> {
        for action_ref in actions {
            let an = action_ref.action.preamble.name;
            let av: Vec<String> = an.split('.').map(|s| s.to_string()).collect();
            let asub = av.last().unwrap();

            if record_name.contains(asub) {
                return Some(an.to_string());
            }
        }

        None
    }
}