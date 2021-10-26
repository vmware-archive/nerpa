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

    let p4info = String::from("../nerpa_controlplane/l2sw/l2sw.p4info.bin");
    let opaque = String::from("../nerpa_controlplane/l2sw/l2sw.json");
    let cookie = String::from("");
    let action = String::from("verify-and-commit");

    // Set the primary controller on P4Runtime, so we can use the StreamChannel RPC.
    let mau_res = p4ext::master_arbitration_update(device_id, &client).await;
    if mau_res.is_err() {
        panic!("could not set master arbitration on switch: {:#?}", mau_res.err());
    }

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

    // TODO: We want to read inputs from both the management plane and the data plane.
    // Currently, this only processes inputs from the data plane.
    nerpa.stream_digests().await;
}
