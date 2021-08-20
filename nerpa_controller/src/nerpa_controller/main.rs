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

use clap::{
    App,
    Arg,
};
use grpcio::{
    ChannelBuilder,
    EnvBuilder
};
use nerpa_controller::Controller;
use proto::p4runtime_grpc::P4RuntimeClient;
use std::sync::Arc;

#[tokio::main]
pub async fn main() {
    const SERVER_ARG: &str = "OVSDB SERVER FILEPATH";
    const SERVER_DEFAULT: &str = "unix:/usr/local/var/run/openvswitch/db.sock";

    const DATABASE_ARG: &str = "OVSDB DB";
    const DATABASE_DEFAULT: &str = "snvs";

    const TARGET_ARG: &str = "P4 TARGET";
    const TARGET_DEFAULT: &str = "localhost:50051";

    const P4INFO_ARG: &str = "P4INFO FILE";
    const P4INFO_DEFAULT: &str = "../nerpa_controlplane/snvs_exp/snvs_p4/snvs.p4info.bin";

    const JSON_ARG: &str = "P4 JSON";
    const JSON_DEFAULT: &str = "../nerpa_controlplane/snvs_exp/snvs_p4/snvs.json";

    let matches = App::new("nerpa_controller")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Read input DDlog relations from OVSDB and push outputs to the P4 dataplane")
        .arg(
            Arg::with_name(SERVER_ARG)
                .help("OVSDB server sockfile path")
                .default_value(SERVER_DEFAULT)
                .index(1),
        )
        .arg(
            Arg::with_name(DATABASE_ARG)
                .help("OVSDB database name")
                .default_value(DATABASE_DEFAULT)
                .index(2),
        )
        .arg(
            Arg::with_name(TARGET_ARG)
                .help("P4 target server address")
                .default_value(TARGET_DEFAULT)
                .index(3),
        )
        .arg(
            Arg::with_name(P4INFO_ARG)
                .help("P4info filepath")
                .default_value(P4INFO_DEFAULT)
                .index(4),
        )
        .arg(
            Arg::with_name(JSON_ARG)
                .help("P4 JSON filepath")
                .default_value(JSON_DEFAULT)
                .index(5),
        )
        .get_matches();

    ovsdb_to_p4(
        matches.value_of(SERVER_ARG).unwrap().to_string(),
        matches.value_of(DATABASE_ARG).unwrap().to_string(),
        matches.value_of(TARGET_ARG).unwrap().to_string(),
        matches.value_of(P4INFO_ARG).unwrap().to_string(),
        matches.value_of(JSON_ARG).unwrap().to_string(),
    );
}

pub fn ovsdb_to_p4(
    server: String,
    database: String,
    target: String,
    p4info_fn: String,
    json_fn: String,
) {
    // Connect the OVS database management plane to the controller.
    let delta = ovsdb_client::export_input_from_ovsdb(server, database).unwrap();

    println!("\n\nProcessed input from OVSDB! Got the following output...");
    let mut controller = Controller::new(delta);
    controller.dump_delta();

    // TODO: Get non-empty election ID working.
    let device_id : u64 = 0;
    let role_id: u64 = 0;
    let env = Arc::new(EnvBuilder::new().build());
    let ch = ChannelBuilder::new(env).connect(target.as_str());
    let client = P4RuntimeClient::new(ch);
    
    let cookie_str: &str = "";
    let action_str: &str = "verify-and-commit";

    p4ext::set_pipeline(
        p4info_fn.as_str(),
        json_fn.as_str(),
        cookie_str,
        action_str,
        device_id,
        role_id,
        target.as_str(),
        &client,
    );

    controller.push_outputs_to_switch(
        device_id,
        role_id,
        target.as_str(),
        &client
    ).unwrap_or_else(
        |err| panic!("could not push outputs to switch: {}", err)
    );
}