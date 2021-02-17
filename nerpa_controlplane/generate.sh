#!/bin/bash
if test ! -f "$DDLOG_HOME/lib/ddlog_std.dl"; then
    echo >&2 "$0: \$DDLOG_HOME must point to the ddlog tree"
    exit 1
fi

ddlog -i nerpa.dl &&
(cd nerpa_ddlog && cargo build --release && cd ..) &&
./nerpa_ddlog/target/release/nerpa_cli < nerpa.dat