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

use rusty_fork::rusty_fork_test;
use std::collections::HashMap;

rusty_fork_test! {
    #[test]
    fn set_get_pipeline() {
        let setup = p4ext::TestSetup::new();

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
        let setup = p4ext::TestSetup::new();
    
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
    let setup = p4ext::TestSetup::new();
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
    let setup = p4ext::TestSetup::new();
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
