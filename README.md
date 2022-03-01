[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

# NERPA

NERPA (Network Programming with Relational and Procedural Abstractions) seeks to enable co-development of the control plane and data plane. This details the project's direction and organization.

1. [nerpa_controlplane](nerpa_controlplane): Each subdirectory corresponds with a Nerpa program, with its input files.
- [DDlog program](nerpa_controlplane/snvs/snvs.dl): Serves as the control plane. 
- [P4 program](nerpa_controlplane/snvs/snvs.p4): Serves as the dataplane program. Used by `p4info2ddlog` to generate DDlog output relations.
- [OVSDB schema](nerpa_controlplane/snvs/snvs.ovsschema): Optionally used to set up an OVSDB management plane. The `ovsdb2ddlog` tool uses this to generate input relations.

2. [nerpa_controller](nerpa_controller): An intermediate Rust [program](nerpa_controller/src/main.rs) runs the DDlog program using the generated crate.  It uses the management plane to adapt the DDlog program's input relations. It also pushes the output relations' rows into tables in the P4 switch using [P4runtime](https://p4.org/p4runtime/spec/master/P4Runtime-Spec.html).
Notice that the controller's `Cargo.toml` is uncommitted. This is generated using the `p4info2ddlog` tool, to import the correct crate dependencies.

3. [ovsdb-sys](ovsdb-sys): Bindings to OVSDB, enabling its use as a management plane.

4. [p4ext](p4ext): API above P4Runtime for convenience.

5. [p4info2ddlog](p4info2ddlog): Script that reads a P4 program's P4info and generates DDlog relations for the dataplane program.

6. [proto](proto): Protobufs for P4 and P4Runtime, used to generate Rust code.


## Installation
### Setup

1. Clone the repository and its submodules.
```
git clone --recursive git@github.com:vmware/nerpa.git
```

2. Install Rust using the [appropriate instructions](https://www.rust-lang.org/tools/install), if uninstalled.

3. The required version of `grpcio` requires CMake >= 3.12. The Ubuntu default is 3.10. [Here](https://askubuntu.com/a/865294) are  installation instructions for Ubuntu.

4. We have included an installation script for Ubuntu. This installs all other dependencies and sets necessary environment variables. On a different operating system, you can individually execute the steps.
```
. scripts/install-nerpa.sh
```

### Build
After installing necessary dependencies, you can write Nerpa programs. A Nerpa program consists of a P4 program, a DDlog program, and (optionally) an OVSDB schema.

For organization, place these programs in the same subdirectory of `nerpa_controlplane`, and give them the same name. Ex., `nerpa_controlplane/sample/sample.p4`, `nerpa_controlplane/sample/sample.dl`.

Once these files are written, the Nerpa program can be built through the build script: `./scripts/build-nerpa.sh nerpa_controlplane/sample sample`. You can also individually execute the steps in the build script.

Building the controller program fails at first. This is due to importing the `*_ddlog::run` function in `nerpa_controller/src/main.rs`. That import must change with the Nerpa program's name.

If you are building a new Nerpa program after building a different example (ex., `nerpa_controlplane/previous/`), you may run into Cargo build errors due to conflicting dependencies. One potential source of errors may be the previous program's DDlog crate. Removing it can resolve these issues:

`rm -rf nerpa_controlplane/previous/previous_ddlog`. 

### Run
A built Nerpa program can be run using the runtime script. This script (1) configures and runs a P4 software switch; (2) configures and runs the OVSDB management plane; and (3) runs the controller program. Configuring the software switch requires a `commands.txt` file in the same subdirectory. Configuring the OVSDB management plane requires an OVSDB schema file in the same subdirectory, e.g. `nerpa_controlplane/sample/sample.ovsschema`.

The runtime script's usage is the same as the build script: `./scripts/run-nerpa.sh nerpa_controlplane/sample sample`.

### Test
The snvs sample program includes an automatic test program to check that the MAC learning table functions as expected.  To use it, first build it with:
```
(cd nerpa_controlplane/snvs/ && cargo build)
```
Then start the behavioral model with the `-s` option to enable the automated tests:
```
scripts/run-nerpa.sh -s nerpa_controlplane/snvs snvs
```
Once it's started (which takes about 2 seconds), from another console run the tests:
```
nerpa_controlplane/snvs/target/debug/test-snvs ipc://bmv2.ipc
```
The test will print its progress.  If it succeeds, it will print `Success!`  On failure, it will panic before that point.

## Writing a Nerpa Program

### Assumptions
The Nerpa programming framework embeds some assumptions about the structure within P4 and DDlog programs. These are documented below.

* Multicast: a DDlog output relation meant to push multicast group IDs to the switch must contain "multicast" in its name (not case-sensitive). A multicast relation must have two records, one representing the group ID and one representing the port. The group ID record name should include "id" (not case-sensitive). The port record name should include "port" (not case-sensitive).

* PacketOut: a DDlog output relation can contain packets to send as [PacketOut messages](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-packet-i_o) over the P4 Runtime API. Such a relation must be a `NamedStruct`, and its name must contain "packet" (not case-sensitive). One of its output Records must represent the packet to send as an `Array`; its name should include "packet" (not case-sensitive). All other fields represent packet metadata fields in the PacketOut struct (the P4 struct with controller header `packet_out`). 