#!/bin/bash

echo "Building Nerpa from CI script..."

# Set environment variables.
NERPA_DIR=$(pwd)
echo "NERPA_DIR: {$NERPA_DIR}"

mkdir nerpa-deps
NERPA_DEPS=$NERPA_DIR/nerpa-deps
cd $NERPA_DEPS
echo "NERPA_DEPS: {$NERPA_DEPS}"


# Install DDlog.
wget https://github.com/vmware/differential-datalog/releases/download/v0.50.0/ddlog-v0.50.0-20211020154401-Linux.tar.gz
tar -xzvf ddlog-v0.50.0-20211020154401-Linux.tar.gz
export PATH=$PATH:$NERPA_DEPS/ddlog/bin
export DDLOG_HOME=$NERPA_DEPS/ddlog

# Install pre-compiled 'protoc' binaries.
PROTOC_URL="https://github.com/protocolbuffers/protobuf/releases"
PROTOC_FN="protoc-3.15.8-linux-x86_64.zip"
wget $PROTOC_URL/download/v3.15.8/$PROTOC_FN
unzip $PROTOC_FN -d $HOME/.local
export PATH="$PATH:$HOME/.local/bin"


# Build 'proto' crate.
echo "Building proto crate..."
cd $NERPA_DIR/proto
cargo install protobuf-codegen
cargo install grpcio-compiler
# cargo build

# Define program-specific variables.
TEST_FN=ci
TEST_DIR=$NERPA_DIR/nerpa_controlplane/ci

# Run 'p4info2ddlog' script.
echo "Generating DDlog relations for dataplane using P4 info..."
cd $NERPA_DIR/p4info2ddlog
cargo run $TEST_DIR $TEST_FN $NERPA_DIR/digest2ddlog

# Compile DDlog crate.
echo "Building Nerpa program DDlog crate..."
cd $NERPA_DIR/nerpa_controlplane/ci
ddlog -i ci.dl &&
(cd ci_ddlog && cargo build --release && cd ..)

# Build Nerpa controller crate.
echo "Building Nerpa controller..."
(cd $NERPA_DIR/nerpa_controller && cargo build && cd ..)