#!/bin/bash
ddlog -i nerpa.dl &&
(cd nerpa_ddlog && cargo build --release && cd ..) &&
./nerpa_ddlog/target/release/nerpa_cli < nerpa.dat