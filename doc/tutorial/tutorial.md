# A Nerpa Tutorial

## Introduction

Nerpa, short for "Network Programming with Relational and Procedural Abstractions", is a programming framework to simplify the management of a programmable network. It implements an incremental control plane and allows for tighter integration and co-design of the control plane and data plane.

In this tutorial we demonstrate how to write and run a Nerpa program.

## Setup

Installation instructions are found in the [README](../../README.md/#installation). To verify installation, confirm that the following environment variables are set:
* `$NERPA_DEPS`, to the directory containing the `behavioral-model` directory.
* `$DDLOG_HOME`, to the DDlog installation directory. Make sure `$PATH` includes the DDlog binary.

## Write a Nerpa Program

### Problem: VLAN Assignment
In this tutorial, we'll create a new Nerpa program called `tutorial`.  We will demonstrate how to write, build, and run a Nerpa program. This program will implement VLAN assignment, which assigns ports to VLANs.

### Create a New Program
We start by creating a new program. Note that all commands should be run from the top-level `nerpa` directory. Run the [creation script](../../scripts/create-new-nerpa.sh):
```
./scripts/create-new-nerpa.sh tutorial
```

Alternately, you can execute its instructions by hand.

This should create a Nerpa program in `nerpa_controlplane/tutorial`. A Nerpa program consists of a DDlog program, ending in `.dl`; a P4 program ending in `.p4`; and an OVSDB schema, ending in `.ovsschema`. It also requires a `commands.txt` file, which is used for initial configuration of the software switch.

Accordingly, before moving forward, make sure that the following directory structure exists:
```
nerpa_controlplane/tutorial
| +-- commands.txt
| +-- tutorial.dl
| +-- tutorial.ovsschema
| +-- tutorial.p4
```

These files should have the following contents.

* `commands.txt` should be empty.

* `tutorial.dl` should only contain the following comments.
```
// Uncomment the following imports after running p4info2ddlog and generating relations from the P4 program and OVSDB schema.
// import tutorial_dp as tutorial_dp
// import Tutorial_mp as tutorial_mp
```
* `tutorial.ovsschema` should contain this empty schema.
```
{
    "name": "tutorial",
    "tables": {},
    "version": "1.0.0"
}
```
* `tutorial.p4` should be an empty P4 program. 

### Program the Management Plane
We will first program the management plane by designing the OVSDB schema. This explains the application's goal and is the simplest part of the program. Writing it down carefully ensures that you understand the problem at hand.

Recall that for VLAN assignment, the input is simply the port. We represent a port using its ID; the type of VLAN; a tag; trunks; and priority.

Copy the contents of [tutorial.ovsschema](tutorial.ovsschema) into `nerpa_controlplane/tutorial/tutorial.ovsschema`.

We can then use the OVSDB schema to generate the DDlog input relations. From the top-level `nerpa` directory:
```
ovsdb2ddlog --schema-file=nerpa_controlplane/tutorial/tutorial.ovsschema --output-file=nerpa_controlplane/tutorial/Tutorial_mp.dl
```

Compare the output file with [Tutorial_mp.dl](Tutorial_mp.dl) to verify its contents.

### Program the Data Plane
To program the data plane, we write the P4 program. This can be quite tricky: P4 is a low-level language with many restrictions, and the DDlog program must cater to those restrictions far more than the other direction.

Copy the contents of [tutorial.p4](tutorial.p4) into `nerpa_controlplane/tutorial/tutorial.p4`.

Compile the P4 program, and generate P4Runtime files:
```
cd nerpa_controlplane/tutorial
p4c --target bmv2 --arch v1model --p4runtime-files tutorial.p4info.bin,tutorial.p4info.txt tutorial.p4
cd ../..
```

Use the `p4info2ddlog` tool to generate relations and helper functions from the compiled P4 program:
```
cd p4info2ddlog
cargo run ../nerpa_controlplane/tutorial tutorial ../dp2ddlog
cd ..
```

Compare the output file with [tutorial_dp.dl](tutorial_dp.dl) to verify its contents.

### Program the Control Plane
To program the control plane, we write the DDlog program that sits in between OVSDB and the P4 switch. Because we have generated the input and output relations, we know what the inputs and outputs to the control plane look like. The DDlog program serves as glue to connect them.

Copy the contents of [tutorial.dl](tutorial.dl) into `nerpa_controlplane/tutorial/tutorial.dl`.

Compile the DDlog program, and build the generated crate:
```
cd nerpa_controlplane/tutorial
ddlog -i tutorial.dl
cd tutorial_ddlog
cargo build
cd ../../..
```

### Build the Nerpa Program
Now that each sub-program exists, we can build the Nerpa tutorial program end-to-end:
```
./scripts/build-nerpa.sh nerpa_controlplane/tutorial tutorial
```

This should successfully build all necessary crates, including `nerpa_controller`.

### Run the Nerpa Program

Run the Nerpa program, starting all pieces of software:
```
./scripts/run-nerpa.sh nerpa_controlplane/tutorial tutorial
```

### Test the Nerpa Program

TODO: Add steps to send a packet and test the tutorial.