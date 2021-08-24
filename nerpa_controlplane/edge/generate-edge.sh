#!/bin/bash
if test ! -f "$DDLOG_HOME/lib/ddlog_std.dl"; then
    echo >&2 "$0: \$DDLOG_HOME must point to the ddlog tree"
    exit 1
fi

# Generate DDlog input relations from OVS schema.
ovsdb2ddlog -f edge.ovsschema --output-file=Edge_mp.dl

# Compile P4 program.
cd edge_p4 && ./run-p4c.sh && cd ..

# Generate DDlog output relations from P4info.
cd ../../p4info2ddlog
cargo run ../nerpa_controlplane/edge/edge_p4/edge.p4info.bin ../nerpa_controlplane/edge/edge_dp.dl
cd ../nerpa_controlplane/edge

# Generate DDlog crate.
ddlog -i edge.dl &&
(cd edge_ddlog && cargo build --release && cd ..)
