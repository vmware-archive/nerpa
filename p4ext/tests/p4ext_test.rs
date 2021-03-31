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

extern crate p4ext;

use grpcio::{ChannelBuilder, EnvBuilder};
use proto::p4runtime::StreamMessageRequest;
use proto::p4runtime_grpc::P4RuntimeClient;
use rusty_fork::fork;
use rusty_fork::rusty_fork_test;
use rusty_fork::rusty_fork_id;
use std::collections::HashMap;
use std::process::Command;
use std::string::String;
use std::sync::Arc;

struct Setup {
    p4info: String,
    opaque: String,
    cookie: String,
    action: String,
    device_id: u64,
    role_id: u64,
    target: String,
    client: P4RuntimeClient,
    table_name: String,
    action_name: String,
    params_values: HashMap<String, u16>,
    match_fields_map: HashMap<String, u16>,
}

impl Setup {
    fn new() -> Self {
        // TODO: Remove hardcoded absolute directory, maybe using symlink?
        let filepath = "/home/dsur/nerpa-deps/behavioral-model/targets/simple_switch_grpc/simple_switch_grpc";
        let mut command = Command::new(filepath);
        command.args(&[
            "--no-p4",
            "--",
            "--grpc-server-addr",
            "0.0.0.0:50051",
            "--cpu-port",
            "1010"
        ]);
        
        match command.spawn() {
            Ok(child) => println!("Server process id: {}", child.id()),
            Err(e) => panic!("server didn't start: {}", e),
        }

        let target = "localhost:50051";
        let env = Arc::new(EnvBuilder::new().build());
        let ch = ChannelBuilder::new(env).connect(target);
        let client = P4RuntimeClient::new(ch);

        let mut params_values : HashMap<String, u16> = HashMap::new();
        params_values.insert("port".to_string(), 11);
        let mut match_fields_map : HashMap<String, u16> = HashMap::new();
        match_fields_map.insert("standard_metadata.ingress_port".to_string(), 11);
        match_fields_map.insert("hdr.vlan.vid".to_string(), 1);


        Self {
            p4info: "examples/vlan/vlan.p4info.bin".to_string(),
            opaque: "examples/vlan/vlan.json".to_string(),
            cookie: "".to_string(),
            action: "verify-and-commit".to_string(),
            device_id: 0,
            role_id: 0,
            target: target.to_string(),
            client: client,
            table_name: "MyIngress.vlan_incoming_exact".to_string(),
            action_name: "MyIngress.vlan_incoming_forward".to_string(),
            params_values: params_values,
            match_fields_map: match_fields_map,
        }
    }
}


rusty_fork_test! {
    #[test]
    fn set_get_pipeline() {
        let setup = Setup::new();

        p4ext::set_pipeline(
            &setup.p4info,
            &setup.opaque,
            &setup.cookie,
            &setup.action,
            setup.device_id,
            setup.role_id,
            &setup.target,
            &setup.client,
        );

        let cfg = p4ext::get_pipeline_config(setup.device_id, &setup.target, &setup.client);
        let switch : p4ext::Switch = cfg.get_p4info().into();
        assert_eq!(switch.tables.len(), 4);
    }
}

rusty_fork_test! {
    #[test]
    fn build_table_entry() {
        let setup = Setup::new();
    
        p4ext::set_pipeline(
            &setup.p4info,
            &setup.opaque,
            &setup.cookie,
            &setup.action,
            setup.device_id,
            setup.role_id,
            &setup.target,
            &setup.client,
        );
    
        // all valid arguments
        assert!(p4ext::build_table_entry(
            &setup.table_name,
            &setup.action_name,
            &setup.params_values,
            &setup.match_fields_map,
            setup.device_id,
            &setup.target,
            &setup.client,
        ).is_ok());
    
        // invalid table name
        assert!(p4ext::build_table_entry(
            "",
            &setup.action_name,
            &setup.params_values,
            &setup.match_fields_map,
            setup.device_id,
            &setup.target,
            &setup.client,
        ).is_err());
    
        // invalid action name
        assert!(p4ext::build_table_entry(
            &setup.table_name,
            "",
            &setup.params_values,
            &setup.match_fields_map,
            setup.device_id,
            &setup.target,
            &setup.client,
        ).is_err());

        // no field matches
        assert!(p4ext::build_table_entry(
            &setup.table_name,
            &setup.action_name,
            &setup.params_values,
            &HashMap::new(),
            setup.device_id,
            &setup.target,
            &setup.client,
        ).is_err());
    }
}

#[tokio::test]
async fn write_read() {
    // TODO: Run in child process.
    let setup = Setup::new();
    p4ext::set_pipeline(
        &setup.p4info,
        &setup.opaque,
        &setup.cookie,
        &setup.action,
        setup.device_id,
        setup.role_id,
        &setup.target,
        &setup.client,
    );

    // Write a table entry.
    let update_result = p4ext::build_table_entry_update(
        proto::p4runtime::Update_Type::INSERT,
        &setup.table_name,
        &setup.action_name,
        &setup.params_values,
        &setup.match_fields_map,
        setup.device_id,
        &setup.target,
        &setup.client,
    );
    assert!(update_result.is_ok());
    let update = update_result.unwrap();

    assert!(p4ext::write(
        [update.clone()].to_vec(),
        setup.device_id,
        setup.role_id,
        &setup.target,
        &setup.client
    ).is_ok());
    let write_entities = [update.clone().take_entity()].to_vec();

    // Set the ReadRequest entity with an empty table entry.
    // This will return all entities containing table entries.
    // That should equal the vector of entries inputted in write().
    let mut read_input_entity = proto::p4runtime::Entity::new();
    read_input_entity.set_table_entry(proto::p4runtime::TableEntry::new());
    let read_result = p4ext::read(
        [read_input_entity].to_vec(),
        setup.device_id,
        &setup.client,
    ).await;
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap().to_vec(), write_entities);
}

#[tokio::test]
async fn stream_channel() {
    let setup = Setup::new();
    p4ext::set_pipeline(
        &setup.p4info,
        &setup.opaque,
        &setup.cookie,
        &setup.action,
        setup.device_id,
        setup.role_id,
        &setup.target,
        &setup.client,
    );

    let master_result = p4ext::master_arbitration_update(
        setup.device_id,
        &setup.client,
    );
    assert!(master_result.await.is_ok());
}
