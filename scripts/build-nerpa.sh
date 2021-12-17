#!/bin/bash
# Script that builds a Nerpa program

# Exit when any command fails, since they are all sequential.
set -e

# Print usage if incorrectly invoked.
if [ "$#" -ne 2 ] || ! [ -d "$1" ]; then
    echo "USAGE: $0 FILE_DIR FILE_NAME" >&2
    echo "* FILE_DIR: directory containing *.p4, *.dl, and optional *.ovsschema files"
    echo "* FILE_NAME: name of the *p4, *dl, and *ovsschema files"
    exit 1
fi

if [[ -z $NERPA_DEPS || -z $DDLOG_HOME ]]; then
    echo "Missing required environment variable (NERPA_DEPS or DDLOG_HOME)"
    echo "Run '. install-nerpa.sh' to set these variables."
    exit 1
fi

echo "Building a Nerpa program..."

export NERPA_DIR=$(pwd)
export FILE_DIR=$NERPA_DIR/$1
export FILE_NAME=$2

cd $FILE_DIR

# Optionally, create a management plane.
# Generate DDlog input relations from the OVSDB schema
if test -f $FILE_NAME.ovsschema; then
    echo "Generating DDlog input relations from OVSDB schema..."
    ovsdb2ddlog -f $FILE_NAME.ovsschema --output-file=${2^}_mp.dl
fi

# Compile P4 program.
if test ! -f "$FILE_NAME.p4"; then
    echo >&2 "$0: could not find P4 program $FILE_NAME.p4 in $1"
    exit 1
fi

echo "Compiling P4 program..."
p4c --target bmv2 --arch v1model --p4runtime-files $FILE_NAME.p4info.bin,$FILE_NAME.p4info.txt $FILE_NAME.p4

# Generate DDlog dataplane relations from P4info, using p4info2ddlog.
echo "Generating DDlog relations for dataplane using P4 info..."
cd $NERPA_DIR/p4info2ddlog
cargo run $FILE_DIR $FILE_NAME $NERPA_DIR/digest2ddlog
cd $FILE_DIR

# Compile DDlog crate.
if test ! -f "$DDLOG_HOME/lib/ddlog_std.dl"; then
    echo >&2 "$0: \$DDLOG_HOME must point to the ddlog tree"
    exit 1
fi

if test ! -f "$FILE_NAME.dl"; then
    echo >&2 "$0: could not find DDlog program $FILE_NAME.dl in $1"
    exit 1
fi

echo "Compiling DDlog crate..."
ddlog -i $FILE_NAME.dl &&
(cd ${FILE_NAME}_ddlog && cargo build --release && cd ..)

# Optionally, generate necessary code for the management plane.
# Build the OVSDB client crate, which depends on the DDlog crate.
if test -f $FILE_NAME.ovsschema; then
    echo "Building the OVSDB client crate..."

    # Generate a `.toml` for the OVSDB client crate.
    cd $NERPA_DIR
    ./scripts/ovsdb-client-toml.sh $1 $2

    # Build the ovsdb client crate.
    cd $NERPA_DIR/ovsdb_client
    pip3 install -r requirements.txt
    python3 ovsdb2ddlog2rust --schema-file=$FILE_DIR/$FILE_NAME.ovsschema -p nerpa_ --output-file src/context/nerpa_rels.rs
    cargo build
    cd $FILE_DIR

    # Create the database from OVSDB schema, if it does not exist.
    # Set up OVSDB. This assumes the Linux default directory.
    OVSDB_FN=/usr/local/etc/openvswitch/$FILE_NAME.db
    if test ! -f $OVSDB_FN; then
        export PATH=$PATH:/usr/local/share/openvswitch/scripts
        ovsdb-tool create $OVSDB_FN $FILE_DIR/$FILE_NAME.ovsschema
    fi
fi

echo "Building controller crate..."
(cd $NERPA_DIR/nerpa_controller && cargo build && cd $NERPA_DIR)
