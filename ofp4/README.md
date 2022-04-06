# What is ofp4?

ofp4 provides a P4Runtime interface to Open vSwitch.  It accepts
P4Runtime connections from a controller and connects to an Open
vSwitch instance over OpenFlow and OVSDB.

ofp4 is an experimental prototype.  Currently, the translation from P4
to OpenFlow is handwritten and has only been done for
`nerpa_controlplane/snvs/snvs.p4`.  You can find this handwritten
translation in `snvs.dl`.  This translation has to be automated by
writing a p4c backend, but that work hasn't been started yet.

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

# Related Work

The P4 OpenFlow Agent (https://github.com/p4lang/p4ofagent) provides
an OpenFlow interface to a P4 switch, the opposite of ofp4.
