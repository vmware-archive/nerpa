# A Nerpa Tutorial

## Introduction

Nerpa, short for "Network Programming with Relational and Procedural Abstractions", is a programming framework to simplify the management of a programmable network. It implements an incremental control plane and allows for tighter integration and co-design of the control plane and data plane.

In this tutorial we demonstrate how to write and run a Nerpa program.

## Nerpa at a high level

![Nerpa vision](nerpa_vision.png)

A Nerpa program consists of three sub-programs, each corresponding to a plane of an enterprise network.
* The management plane sets high-level policy for the network devices. We use Open vSwitch Database (OVSDB) for the management plane. An OVSDB schema initializes the management plane. The admin user can insert, modify, or delete rows in the database to represent changes in high-level configuration.
* The control plane configures the data plane based on the declared state of the network. We use a Differential Datalog (DDlog) program for the control plane. A DDlog program consists of rules that compute a set of output relations based on input relations. These rules are evaluated incrementally: given a set of changes to the input relations, DDlog produces a set of changes to the output relations. Those output relations are written to the P4-enabled switch.
* The data plane processes packets that pass through the system. We program this using a P4 program. A P4 program specifies how data plane devices, like switches and routers, process packets.

DDlog input and output relations are generated from the OVSDB schema and P4 program. They are imported by the DDlog program that is compiled and used as the control plane. This facilitates codesign and tighter integration of the control and data planes.

The Nerpa controller synchronizes state between the planes. It does the following:
* Listens for changes from the OVSDB management plane; converts them into DDlog input relations; and sends them to the control plane
* Receives DDlog output relations from the control plane; converts them into P4 entries; and sends them to the data plane
* Listens for digest notifications from the P4 data plane; converts them into DDlog input relations; and sends them to the control plane

## Setup

Installation instructions are found in the [README](../../README.md/#installation). To verify installation, confirm that the following environment variables are set:
* `$NERPA_DEPS`, to the directory containing the `behavioral-model` directory.
* `$DDLOG_HOME`, to the DDlog installation directory. Make sure `$PATH` includes the DDlog binary.

## Write a Nerpa Program

### Problem: VLAN Assignment
In this tutorial, we'll create a new Nerpa program called `tutorial`.  We will demonstrate how to write, build, and run a Nerpa program. This program will implement VLAN assignment, which assigns ports to VLANs. A port is represented using its ID; the type of VLAN; a tag; trunks; and priority.

Translating this to DDlog, the input relations would represent ports, and the output relations represent the assigned VLANs.

Below, we instantiate the system diagram above for the `tutorial` example.

![Tutorial example](tutorial_impl_diagram.png)

### Create a New Program
We start by creating a new program. Note that all commands should be run from the top-level `nerpa` directory. Run the [creation script](../../scripts/create-new-nerpa.sh):
```
./scripts/create-new-nerpa.sh tutorial
```

Alternately, you can execute the script instructions manually one-by-one.

This should create a Nerpa program in `nerpa_controlplane/tutorial`, composed of the following files:
* `tutorial.dl`: Datalog program implementing the control plane, that will run on a centralized controller
* `tutorial.p4`: P4 program implementing the data plane, that will run on a software switch
* `tutorial.ovsschema`: OVSDB schema specifying the management plane 
* `commands.txt`: commands sent to the P4 switch's command-line interface and used to initialize the control plane

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

We pass the OVSDB schema as input to the `ovsdb2ddlog` tool, which generates DDlog relations from the schema. This helps us directly read changes from OVSDB and convert them to inputs for the running DDlog program.

To do this, copy the contents of [tutorial.ovsschema](tutorial.ovsschema) into `nerpa_controlplane/tutorial/tutorial.ovsschema`. Then, run the following command. This, and all other commands in this tutorial, should be run from the top-level `nerpa` directory:
```
ovsdb2ddlog --schema-file=nerpa_controlplane/tutorial/tutorial.ovsschema --output-file=nerpa_controlplane/tutorial/Tutorial_mp.dl
```

Compare the output file with [Tutorial_mp.dl](Tutorial_mp.dl) to verify its contents.

### Program the Data Plane
To program the data plane, we write the P4 program. `tutorial.p4` specifies how packets with a VLAN header should be processed. P4 is a low-level language with many restrictions, and the DDlog program must cater to those restrictions far more than the other direction.

Copy the contents of [tutorial.p4](tutorial.p4) into `nerpa_controlplane/tutorial/tutorial.p4`.

Compile the P4 program, and generate P4Runtime files:
```
cd nerpa_controlplane/tutorial
p4c --target bmv2 --arch v1model --p4runtime-files tutorial.p4info.bin,tutorial.p4info.txt tutorial.p4
cd ../..
```

Use the `p4info2ddlog` tool to generate relations and helper functions from the compiled P4 program. Generating relations ensures that DDlog output relations can be converted to P4 match-action tables, and that P4 digests can be converted to DDlog input relations. Helper functions facilitate this type conversion within the Nerpa codebase.
```
cd p4info2ddlog
cargo run ../nerpa_controlplane/tutorial tutorial ../dp2ddlog
cd ..
```

Compare the output file with [tutorial_dp.dl](tutorial_dp.dl) to verify its contents.

### Program the Control Plane
To program the control plane, we write the DDlog program that sits in between OVSDB and the P4 switch. Because we have generated the input and output relations, we know what the inputs and outputs to the control plane look like. The DDlog program connects these and implements the control plane's actions, by computing output changes from the input changes.

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