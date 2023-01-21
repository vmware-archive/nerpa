# Compiling P4 to OpenFlow

This directory contains a compiler that converts P4 code to OpenFlow.
The target for this compiler is of_model.p4, which can be found in the
`p4include` directory.

The compiler in fact consumes a P4 program and generates a
Differential Datalog (DDlog) program
(https://github.com/vmware/differential-datalog).  The DDlog program
implements a controller called ofp4, which controls an OpenFlow
programmable device.

Currently, the translation from P4 to DDlog has been handwritten for
`nerpa_controlplane/snvs/snvs.p4`.  You can find this handwritten
translation in `snvs.dl`.  Eventually the compiler will generate an
equivalent form.

## Compiling the p4c-of compiler

To build the P4 compiler with this backend you have to perform the
following steps:

* Check out the p4c repository.  To make sure that everything is OK
  and to prepare for the rest of the process, configure and build it
  according to the instructions in the tree.  This should amount to
  running `cmake` then `make`.

* Create a symbolic link in the directory p4c/extensions pointing to
  this directory.  For example, if p4c is checked out into `p4c` and
  Nerpa into `nerpa`:

```
mkdir p4c/extensions
ln -s $(pwd)/nerpa/ofp4 p4c/extensions
```

* Rebuild p4c compiler the same way you did before.  If you created
  the link properly in the previous step, the build will automatically
  detect and build the extension.

* The result of the compilation should be a `p4c-of` binary.  Install
  it, e.g. with `make install`.

## What is ofp4?

ofp4 is essentially a controller with a P4Runtime interface that
controls Open vSwitch.  It accepts P4Runtime connections from a
controller and connects to an Open vSwitch instance over OpenFlow and
OVSDB.

For more information about ofp4, please see the P4 Workshop 2022
presentation.  The [extended abstract](p4-workshop-paper.pdf) and
[slides](p4-workshop-slides.pdf) for this talk are available in this
directory, as well as
[video](https://www.youtube.com/watch?v=OpBa7s8EcLg) on YouTube.

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

## How to use ofp4

After installing `p4c-of` as described above:

1. Compile your P4 source to DDlog, e.g. with `p4c-of <name>.p4 -o
   <name>.dl`.

2. Edit `ofp4dl.dl` to import `<name>.dl`, e.g. by adding `import
   <name>`.  This file can import any number of `p4c-of`-generated
   DDlog files, so you don't have to remove the ones that are already
   imported.
   
3. Compile the DDlog to Rust: `ddlog -i ofp4dl.dl`.

4. Build `ofp4`, e.g. with `cargo build`.

5. Run `ofp4`, telling it to use the P4 program you compiled,
   e.g. with `cargo run <name> <ovs>`, where `<ovs>` tells `ofp4` how
   to connect to a running OVS bridge and would most commonly start
   with `unix:` to connect to a local OVS process.

   By default, ofp4 listens on 127.0.0.1:50051 for P4Runtime
   connections (use `--p4-port` and `--p4-addr` command-line options
   to override these defaults).

   As an alternative to step 4, instead of running `ofp4` directly,
   pass `--ofp4` to `scripts/run-nerpa.sh` to make it start up OVS and
   ofp4 instead of bmv2.  This won't pass the tests, since MAC
   learning won't work yet.

## Related Work

The P4 OpenFlow Agent (https://github.com/p4lang/p4ofagent) provides
an OpenFlow interface to a P4 switch, the opposite of ofp4.
