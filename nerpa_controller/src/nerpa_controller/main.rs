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

extern crate nerpa_controller;
extern crate grpcio;
extern crate proto;
extern crate protobuf;

use differential_datalog::ddval::DDValConvert;
use differential_datalog::program::{RelId, Update};
use grpcio::{ChannelBuilder, EnvBuilder};
use nerpa_controller::{
    Controller,
    SwitchClient
};
use proto::p4runtime_grpc::P4RuntimeClient;
use std::sync::Arc;

#[tokio::main]
pub async fn main() {
    // TODO: Stop hard-coding arguments.
    // TODO: Get non-empty election ID working.
    let device_id : u64 = 0;
    let role_id: u64 = 0;
    let target = String::from("localhost:50051");
    let env = Arc::new(EnvBuilder::new().build());
    let ch = ChannelBuilder::new(env).connect(target.as_str());
    let client = P4RuntimeClient::new(ch);

    // let p4info = String::from("../nerpa_controlplane/snvs_exp/snvs_p4/snvs.p4info.bin");
    // let opaque = String::from("../nerpa_controlplane/snvs_exp/snvs_p4/snvs.json");
    let p4info = String::from("../nerpa_controlplane/l2sw/l2sw.p4info.bin");
    let opaque = String::from("../nerpa_controlplane/l2sw/l2sw.json");
    let cookie = String::from("");
    let action = String::from("verify-and-commit");

    // Create a SwitchClient, to talk to the Switch.
    let switch_client = SwitchClient::new(
        client,
        p4info,
        opaque,
        cookie,
        action,
        device_id,
        role_id,
        target,
    );
    
    // Instantiate DDlog program.
    let nerpa = Controller::new(switch_client).unwrap();

    // TODO: Connect the OVS database management plane to the controller.
    // Add input to DDlog program.
    /*
    let updates = vec![
        Update::Insert{
            relid: Relations::snvs_mp_Port as RelId,
            v: types__snvs_mp::Port {
                _uuid: 0,
                id: 1,
                vlan_mode: ddlog_std::Option::Some{x: "".to_string()},
                tag: ddlog_std::Option::Some{x: 1},
                trunks: ddlog_std::Set::new(),
                priority_tagging: "no".to_string(), 
            }.into_ddvalue(),
        },
    ];

    nerpa.input_to_switch(updates).await.unwrap_or_else(
        |err| panic!("could not push outputs to switch: {}", err)
    ); */

    nerpa.stream_digests().await;
}
