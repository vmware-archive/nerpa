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

use clap::{App, Arg};
use grpcio::{ChannelBuilder, EnvBuilder};
use nerpa_controller::{Controller, SwitchClient};
use proto::p4runtime_grpc::P4RuntimeClient;
use std::sync::Arc;
use std::fs::File;

// Import the function to run a DDlog program.
// Note that the crate name changes with the Nerpa program's name.
// The Nerpa programmer must rename this import.
use snvs_ddlog::run;

#[tokio::main]
pub async fn main() {
    const FILE_DIR_ARG: &str = "file-directory";
    const FILE_NAME_ARG: &str = "file-name";
    const DDLOG_RECORD: &str = "ddlog-record";

    let matches = App::new("nerpa_controller")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Starts the controller program")
        .arg(
            Arg::with_name(FILE_DIR_ARG)
                .help("Directory path with input files (*.p4info.bin, *.json, *.dl)")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name(FILE_NAME_ARG)
                .help("Filename before the extension: {file-name}.p4info.bin, {file-name}.dl")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name(DDLOG_RECORD)
                .long("ddlog-record")
                .takes_value(true)
                .value_name("FILE.TXT")
                .help("File to record DB changes to replay later for debugging"),
        )
        .get_matches();

    // Validate CLI arguments.
    let file_dir_opt = matches.value_of(FILE_DIR_ARG);
    if file_dir_opt.is_none() {
        panic!("missing required argument: file-directory");
    }

    let file_name_opt = matches.value_of(FILE_NAME_ARG);
    if file_name_opt.is_none() {
        panic!("missing required argument: file-name");
    }

    let mut record_file = matches.value_of_os(DDLOG_RECORD).map(
        |filename| match File::create(filename) {
            Ok(file) => file,
            Err(err) => panic!("{}: create failed ({})", filename.to_string_lossy(), err)
        }
    );

    // Run controller.
    let file_dir = String::from(file_dir_opt.unwrap());
    let file_name = String::from(file_name_opt.unwrap());
    run_controller(file_dir, file_name, &mut record_file).await
}

async fn run_controller(
    file_dir: String,
    file_name: String,
    record_file: &mut Option<File>,
) {
    // Run the DDlog program. This computes initial contents to push across switches.
    let (mut hddlog, initial_contents) = run(1, false).unwrap();
    hddlog.record_commands(record_file);

    // Define values that are common across all the switch clients.
    let p4info = format!("{}/{}.p4info.bin", file_dir, file_name);
    let opaque = format!("{}/{}.json", file_dir, file_name);
    let cookie = String::from("");
    let action = String::from("verify-and-commit");

    // Define the switch client-specific configurations.
    // A configuration is of the form (target, device_id, role_id, is_primary).
    let configs = [
        ("localhost:50051", 0, 0, true)
    ];

    let mut switch_clients = Vec::new();

    for config in configs {
        let (target_str, device_id, role_id, is_primary) = config;
        let env = Arc::new(EnvBuilder::new().build());
        let ch = ChannelBuilder::new(env).connect(target_str);
        let client = P4RuntimeClient::new(ch);

        // If primary, set the controller as primary using P4Runtime.
        // This enables use of the StreamChannel RPC.
        if is_primary {
            let mau_res = p4ext::master_arbitration_update(device_id, &client).await;
            if mau_res.is_err() {
                panic!("could not set master arbitration on switch: {:#?}", mau_res.err());
            }
        }

        // Create a SwitchClient.
        // Handles communication with the switch.
        let mut sc = SwitchClient::new(
            client,
            p4info.clone(),
            opaque.clone(),
            cookie.clone(),
            action.clone(),
            device_id,
            role_id,
            String::from(target_str),
        ).await;

        sc.push_ddlog_outputs(&initial_contents).await.unwrap();
        switch_clients.push(sc);
    }

    // Instantiate controller.
    // We store the DDlog program on the heap. This lets us safely pass
    // references to heap memory to both the controller and OVSDB client.
    let controller_hddlog = Arc::new(hddlog);
    let ovsdb_hddlog = controller_hddlog.clone();
    let nerpa_controller = Controller::new(switch_clients, controller_hddlog).unwrap();

    // Start streaming inputs from OVSDB and from the dataplane.
    let server = String::from("unix:nerpa.sock");
    let database = file_name.clone();
    nerpa_controller.stream_inputs(ovsdb_hddlog, server, database).await;
}
