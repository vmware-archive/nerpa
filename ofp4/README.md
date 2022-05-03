# Compiling P4 to Open-Flow

This directory contains a compiler that converts P4 code to Open-Flow.
The target for this compiler is of_model.p4, which can be found in the
`p4include` directory.

The compiler in fact consumes a P4 program and generates a
Differential Datalog (DDlog) program
(https://github.com/vmware/differential-datalog).  The DDlog program
implements a controller called ofp4, which controls an Open-Flow
programmable device.

Currently, the translation from P4 to DDlog has been handwritten for
`nerpa_controlplane/snvs/snvs.p4`.  You can find this handwritten
translation in `snvs.dl`.  Eventually the compiler will generate an
equivalent form.

## Compiling the p4c-of compiler

To build the P4 compiler with this backend you have to perform the
following steps:

* Check out the p4c repository from the above address and make sure you can compile the code
* Create a symbolic link in the directory p4c/extensions pointing to this directory.
For example, if p4c is checked out into `p4c` and Nerpa into `nerpa`:

```
mkdir p4c/extensions
ln -s $(pwd)/nerpa/ofp4 p4c/extensions
```

* Rebuild the p4c compiler using the standard methodology.

The result of the compilation should be a `p4c-of` binary.

## Running tests

To run only the ofp4-specific tests you can use the p4c
testing infrastructure, by invoking

```
make check-of
```

## What is ofp4?

ofp4 is essentially a controller with a P4Runtime interface that
controls Open vSwitch.  It accepts P4Runtime connections from a
controller and connects to an Open vSwitch instance over OpenFlow and
OVSDB.

ofp4 is an experimental prototype.

So far, ofp4 accepts P4Runtime connections and allows multicast groups
and table entries to be updated, translates those into OpenFlow flows
through the rules written in `snvs.dl`, and installs those into Open
vSwitch flow tables using OpenFlow.  ofp4 does not yet support
P4Runtime digests or other ways of sending feedback to the P4Runtime
controller, so it won't be able to support `snvs.p4` MAC learning yet.
ofp4 won't ever be able to support some P4 features, such customizable
parsers and deparsers and most kind of arithmetic, at least not
without adding new Open vSwitch extensions.

To use ofp4, invoke it with an OpenFlow connection method for the Open
vSwitch bridge to connect to as its command-line argument.  By
default, ofp4 listens on 127.0.0.1:50051 for P4Runtime connections
(use `--p4-port` and `--p4-addr` command-line options to override
these defaults).

Pass `--ofp4` to `scripts/run-nerpa.sh` to make it start up OVS and
ofp4 instead of bmv2.  This won't pass the tests, since MAC learning
won't work yet.

## Related Work

The P4 OpenFlow Agent (https://github.com/p4lang/p4ofagent) provides
an OpenFlow interface to a P4 switch, the opposite of ofp4.
