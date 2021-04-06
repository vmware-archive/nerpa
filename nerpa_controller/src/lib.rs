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

// The auto-generated crate `nerpa_ddlog` declares the `HDDlog` type.
// This serves as a reference to a running DDlog program.
// It implements `trait differential_datalog::DDlog`.
use nerpa_ddlog::api::HDDlog;
use nerpa_ddlog::Relations;

// `differential_datalog` contains the DDlog runtime copied to each generated workspace.
use differential_datalog::DDlog; // Trait that must be implemented by DDlog program.
use differential_datalog::DeltaMap; // Represents a set of changes to DDlog relations.
use differential_datalog::ddval::DDValue; // Generic type wrapping all DDlog values.
use differential_datalog::ddval::DDValConvert; // Trait to convert Rust types to/from DDValue.
use differential_datalog::program::RelId;
use differential_datalog::program::Update;

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

    pub fn add_input(&mut self, ports: Vec<types::Port>) -> Result<DeltaMap<DDValue>, String> {
        self.hddlog.transaction_start()?;

        let updates = ports.into_iter().map(|port|
            Update::Insert {
                relid: Relations::Port as RelId,
                v: types::Port{
                    port_id: port.port_id,
                    config: port.config
                }.into_ddvalue(),
            }
        ).collect::<Vec<_>>();

        self.hddlog.apply_valupdates(updates.into_iter())?;
        self.hddlog.transaction_commit_dump_changes()
    }

    pub fn dump_delta(delta: &DeltaMap<DDValue>) {
        for (rel, changes) in delta.iter() {
            println!("Changes to relation {}", nerpa_ddlog::relid2name(*rel).unwrap());
            for (val, weight) in changes.iter() {
                println!("{} {:+}", val, weight);
            }
        }
    }

    fn extract_vlan_match_fields_param_values(v: DDValue) -> (Vec<HashMap<String, u16>>, Vec<HashMap<String, u16>>) {
        let vlan_ports = unsafe { types::VlanPorts::from_ddval( v.into_ddval() )};

        let mut match_vec = Vec::new();
        let mut param_vec = Vec::new();
        for p in vlan_ports.ports {
            let match_fields : HashMap<String, u16> = [
                (String::from("hdr.vlan.vid"), vlan_ports.vlan),
                (String::from("standard_metadata.ingress_port"), p),
            ].iter().cloned().collect();
            match_vec.push(match_fields);

            let param_values : HashMap<String, u16> = [
                (String::from("port"), p)
            ].iter().cloned().collect();
            param_vec.push(param_values);
        }

        (match_vec, param_vec)
    }

    pub fn push_outputs_to_switch(
        delta: &DeltaMap<DDValue>,
        device_id: u64,
        role_id: u64,
        target: &str,
        table_name : &str,
        action_name: &str,
        client: &P4RuntimeClient,
    ) {
        let mut updates = Vec::new();

        for (_rel_id, (_map_size, delta_map)) in (*delta).clone().into_iter().enumerate() {
            for (k, _v) in delta_map {
                let (match_vec, param_vec) = Self::extract_vlan_match_fields_param_values(k.clone());

                for (i, match_fields_map) in match_vec.iter().enumerate() {
                    // Both vectors have the same length, so the below access is safe.
                    let params_values = &param_vec[i];
                    let update = p4ext::build_table_entry_update(
                        proto::p4runtime::Update_Type::INSERT,
                        table_name,
                        action_name,
                        params_values,
                        &match_fields_map,
                        device_id,
                        target,
                        client,
                    ).unwrap_or_else(|err| panic!("could not build table entry update: {}", err));
                    updates.push(update);
                }
            }
        }

        p4ext::write(updates, device_id, role_id, target, client);
    }
}