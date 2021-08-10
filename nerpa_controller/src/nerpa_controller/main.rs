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
use nerpa_controller::Controller;
use proto::p4runtime_grpc::P4RuntimeClient;
use snvs_ddlog::Relations;
use snvs_ddlog::typedefs::ddlog_std;
use std::sync::Arc;

#[tokio::main]
pub async fn main() {
    // Instantiate DDlog program.
    let mut nerpa = Controller::new().unwrap();

    // Connect the OVS database management plane to the controller.
    let server = "unix:/usr/local/var/run/openvswitch/db.sock";
    let database = "snvs";
    let delta = ovsdb_client::export_input_from_ovsdb(server.to_string(), database.to_string()).unwrap();

    println!("\n\nProcessed input from OVSDB! Got following output...");
    Controller::dump_delta(&delta);

    // TODO: Stop hard-coding arguments.
    // TODO: Get non-empty election ID working.
    let device_id : u64 = 0;
    let role_id: u64 = 0;
    let target : &str = "localhost:50051";
    let env = Arc::new(EnvBuilder::new().build());
    let ch = ChannelBuilder::new(env).connect(target);
    let client = P4RuntimeClient::new(ch);

    let p4info_str: &str = "../nerpa_controlplane/snvs_exp/snvs_p4/snvs.p4info.bin";
    let opaque_str: &str = "../nerpa_controlplane/snvs_exp/snvs_p4/snvs.json";
    let cookie_str: &str = "";
    let action_str: &str = "verify-and-commit";

    p4ext::set_pipeline(
        p4info_str,
        opaque_str,
        cookie_str,
        action_str,
        device_id,
        role_id,
        target,
        &client,
    );

    Controller::push_outputs_to_switch(
        &delta,
        device_id,
        role_id,
        target,
        &client,
    ).unwrap_or_else(
        |err| panic!("could not push outputs to switch: {}", err)
    );
}
