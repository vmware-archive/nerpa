[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

# NERPA

NERPA (Network Programming with Relational and Procedural Abstractions) seeks to enable co-development of the control plane and data plane. This document details the project's direction and organization. As of writing, the following parts are mostly unimplemented.

1. [nerpa_controlplane](nerpa_controlplane): A [DDlog program](nerpa_controlplane/nerpa.dl) serves as the control plane. Its input relations are fed from the management plane, and its output relations feed the data plane. After initial setup, this directory will include a generated DDlog crate used by the controller.
2. [nerpa_controller](nerpa_controller): An intermediate Rust [program](nerpa_controller/src/main.rs) runs the DDlog program using the generated crate.  It uses the management plane to adapt the DDlog program's input relations. It also pushes the output relations' rows into tables in the P4 switch using [P4runtime](https://p4.org/p4runtime/spec/master/P4Runtime-Spec.html).
3. nerpa_dataplane: We plan to implement the data plane in [P4](https://p4.org/p4-spec/docs/P4-16-working-spec.html) using the [table format](https://p4.org/p4-spec/docs/P4-16-working-spec.html#sec-tables). Note that this may require cross-language work, as it is unclear if this involves any Rust.

## Installation
### Initial Setup
1. Install DDlog using the provided [installation instructions](https://github.com/vmware/differential-datalog/blob/master/README.md#installation).
2. Generate the DDlog crate using the [setup script](nerpa_controlplane/generate.sh). We do not commit this crate so that small differences in developer toolchains do not create significant hassle.
```
cd nerpa_controlplane
./generate.sh
```
3. Generate and run the intermediate controller program's crate. Note that the input relations are currently hardcoded, as the interface for user interaction with the intermediate controller is unimplemented.
```
cd ../nerpa_controller
cargo build --release && cargo run
```
The result of running the above program should be:
```
Changes to relation Vlans
Vlans{.number = 11, .vlans = [1]} +1
```