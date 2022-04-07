[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

# NERPA

Nerpa, short for "Network Programming with Relational and Procedural Abstractions", is a programming framework to simplify the management of a programmable network. It implements an incremental control plane and allows for tighter integration and co-design of the control plane and data plane.

In our current vision for Nerpa, we interoperate between an Open vSwitch Database (OVSDB) management plane; a Differential Datalog (DDlog) program as control plane; and a P4 program for the data plane. This diagram shows how those pieces interact.

![Nerpa vision](doc/tutorial/nerpa_vision.png)

The Nerpa framework involves several components, located in different subdirectories. This repo is organized as follows:

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

The above pieces fit together as follows in the [`tutorial` Nerpa program](doc/tutorial/tutorial.md):

![Tutorial example](doc/tutorial/tutorial_impl_diagram.png)

## Steps
### Installation

1. Clone the repository and its submodules.
```
git clone --recursive git@github.com:vmware/nerpa.git
```

2. Install Rust using the [appropriate instructions](https://www.rust-lang.org/tools/install), if uninstalled.

3. The required version of `grpcio` requires CMake >= 3.12. The Ubuntu default is 3.10. [Here](https://askubuntu.com/a/865294) are  installation instructions for Ubuntu.

4. We have included an installation script for Ubuntu. This installs all other dependencies and sets necessary environment variables. On a different operating system, you can individually execute the steps. Following the installation script's organization ensures compatibility with the build and runtime scripts.
```
. scripts/install-nerpa.sh
```

### Tutorial
After installing all dependencies, you can write Nerpa programs. We recommend following the [tutorial](doc/tutorial/tutorial.md) for a step-by-step introduction to Nerpa. Individual steps for setup are also documented below. 

### Build
The Nerpa program called `example` would consist of the following files. For organization, these files should be placed in the same subdirectory of `nerpa_controlplane` and given the same name, as follows:
```
nerpa_controlplane/example/example.dl // DDlog program for the controlplane
nerpa_controlplane/example/example.p4 // P4 program for the dataplane
nerpa_controlplane/example/commands.txt // Initial commands to configure the P4 switch
nerpa_controlplane/example/example.ovsschema // Schema for the OVSDB management plane
```

These files can also be created using a convenience script: `./scripts/create-new-nerpa.sh example`.

Note that even though a Nerpa program does not have to use an OVSDB management plane, deleting the `.ovsschema` currently causes downstream build errors.

Once these files are written, the Nerpa program can be built by running the build script: `./scripts/build-nerpa.sh nerpa_controlplane/example example`. You can also individually execute the steps in the build script, as long as DDlog has been installed. Note that we do recommend using the build script, so that all software is in the expected locations for the runtime script.

If you are building a new Nerpa program after building a different example (ex., `nerpa_controlplane/previous/`), you may run into Cargo build errors due to conflicting dependencies. One potential source of errors may be the previous program's DDlog crate. Removing it can resolve these issues:

`rm -rf nerpa_controlplane/previous/previous_ddlog`. 

### Run
A built Nerpa program can be run using the runtime script. This script (1) configures and runs a P4 software switch; (2) configures and runs the OVSDB management plane; and (3) runs the controller program.

The runtime script's usage is the same as the build script:
```
./scripts/run-nerpa.sh nerpa_controlplane/example example
```

If you did not previously use the installation and build scripts, you must ensure that all software dependencies are in the expected locations for the runtime script.

### Test
The snvs example program includes an automatic test program to check that the MAC learning table functions as expected.  To use it, first build it with:
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

### Write/Build Process
Writing and building a Nerpa program involves several steps. We lay those out here for clarity and to reduce pitfalls for new Nerpa programmers. All of these are steps of the build script, `scripts/build-nerpa.sh`. That script includes specific syntax that you should use if you are rolling your own build process.

1. Create default Nerpa program files: a P4 program, a DDlog program, an OVSDB schema, and P4 switch configuration commands. This is described [above](#build).

2. Optionally, design the OVSDB schema for the management plane and generate DDlog relations using `ovsdb2ddlog`.

3. Write the P4 program. Compile it, making sure to generate P4 runtime files.

4. Generate DDlog relations and related utilities from the dataplane program by calling `p4info2ddlog`. Note that running the full build script compiles the stub DDlog program and builds the crate, which can take several minutes.

In order, `p4info2ddlog` does the following:
* Generate DDlog input relations representing P4 tables and actions
* Generate DDlog input relations representing digest messages from P4
* Generate `Cargo.toml` for the `nerpa_controller` crate, so it correctly imports all DDlog-related crates
* Create the `dp2ddlog` crate, which can convert digests and packets to DDlog relations

5. Write the DDlog program. This represents the rules from the control plane. At this point, all relations necessary for import should be generated.

6. Generate necessary files and build the `ovsdb_client` crate. Even if your program does not use an OVSDB management plane, `nerpa_controller` depends on this import.

7. Build the controller crate. The name of the imported DDlog crate does need to be changed before building.

### Assumptions
The Nerpa programming framework embeds some assumptions about the structure within P4 and DDlog programs. These are documented below.

* Multicast: a DDlog output relation meant to push multicast group IDs to the switch must contain "multicast" in its name (not case-sensitive). A multicast relation must have two records, one representing the group ID and one representing the port. The group ID record name should include "id" (not case-sensitive). The port record name should include "port" (not case-sensitive).

* PacketOut: a DDlog output relation can contain packets to send as [PacketOut messages](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-packet-i_o) over the P4 Runtime API. Such a relation must be a `NamedStruct`, and its name must contain "packet" (not case-sensitive). One of its output Records must represent the packet to send as an `Array`; its name should include "packet" (not case-sensitive). All other fields represent packet metadata fields in the PacketOut struct (the P4 struct with controller header `packet_out`).
