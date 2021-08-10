#!/bin/bash
if test ! -f "$DDLOG_HOME/lib/ddlog_std.dl"; then
    echo >&2 "$0: \$DDLOG_HOME must point to the ddlog tree"
    exit 1
fi

# This script assumes the combined DDlog program has been written.
# It generates the input and output relations. 

# Generate DDlog input relations from OVS schema.
ovsdb2ddlog -f snvs.ovsschema --output-file=Snvs_mp.dl

# Compile P4 program,
cd snvs_p4 && ./run-p4c.sh && cd ..

# Generate DDlog output relations from P4info.
cd ../../p4info2ddlog
cargo run ../nerpa_controlplane/snvs_exp/snvs_p4/snvs.p4info.bin ../nerpa_controlplane/snvs_exp/snvs_dp.dl
cd ../nerpa_controlplane/snvs_exp

# Generate DDlog crate.
ddlog -i snvs.dl &&
(cd snvs_ddlog && cargo build --release && cd ..)