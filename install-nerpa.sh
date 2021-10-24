#!/bin/bash
# Install Nerpa and dependencies on Linux

export NERPA_DIR=$(pwd)

mkdir nerpa-deps
export NERPA_DEPS=$NERPA_DIR/nerpa-deps
cd $NERPA_DEPS

# Install DDlog.
echo "Installing DDlog..."
if [[ -z $DDLOG_HOME ]]; then
    wget https://github.com/vmware/differential-datalog/releases/download/v0.39.0/ddlog-v0.39.0-20210411172417-linux.tar.gz
    tar -xzvf ddlog-v0.39.0-20210411172417-linux.tar.gz
    export PATH=$PATH:$NERPA_DEPS/ddlog/bin
    export DDLOG_HOME=$NERPA_DEPS/ddlog
fi

# Install P4 with dependency software.
echo "Installing P4 software..."
git clone https://github.com/jafingerhut/p4-guide
./p4-guide/bin/install-p4dev-v2.sh |& tee log.txt

# Configure PI.
echo "Configuring PI..."
CONFIGURE="./configure --prefix=$NERPA_DEPS/inst CPPFLAGS=-I$NERPA_DEPS/inst/include LDFLAGS=-L$NERPA_DEPS/inst/lib"
(cd PI && ./autogen.sh && $CONFIGURE --with-proto)

# Configure the behavioral model switch.
echo "Configuring behavioral model..."
(cd behavioral-model && autogen.sh && $CONFIGURE --with-pi)
(cd behavioral-model && make install)
(cd behavioral-model/targets/simple_switch_grpc/ && ./autogen.sh && $CONFIGURE && make install)

# Configure the p4c compiler.
echo "Configuring p4c..."
mkdir p4c/build
(cd p4c/build && cmake -DCMAKE_INSTALL_PREFIX:PATH=$NERPA_DEPS/inst .. && make)

# Build `proto` crate.
echo "Building proto crate..."
cd $NERPA_DIR/proto
cargo install protobuf-codegen
cargo install grpcio-compiler
cargo build
cd $NERPA_DIR

# Build the OVSDB bindings crate.
cd $NERPA_DIR/ovsdb-sys
git submodule update --init

# Install OVS.
cd ovs
./boot.sh
./configure
make
make install

# Build the crate.
cargo build
cargo test
