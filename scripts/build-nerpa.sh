#!/bin/bash
# Script that builds a Nerpa program

# Exit when any command fails, since they are all sequential.
set -e

# Print usage if incorrectly invoked.
if [ "$#" -ne 2 ] || ! [ -d "$1" ]; then
    cat >&2 <<EOF
Usage: $0 FILE_DIR FILE_NAME
where FILE_DIR contains *.p4, *.dl, and *.ovsschema files
  and FILE_NAME is the name of the *.p4, *.dl, and *.ovsschema files.
EOF
    exit 1
fi

# Check if the Nerpa dependencies were installed correctly.
if [[ -z $NERPA_DEPS ]]; then
    NERPA_DEPS=$(pwd)/nerpa-deps

    # If the Nerpa dependencies directory exists, set the environment variables.
    if [[ -d $NERPA_DEPS ]]; then
        export NERPA_DEPS

        # Check if the DDlog variables are set.
        if [[ -z $DDLOG_HOME ]]; then
            # If DDlog is found within the Nerpa dependency directory, set the DDlog variables.
            if [[ -d $NERPA_DEPS/ddlog ]]; then
                export DDLOG_HOME=$NERPA_DEPS/ddlog
                export PATH=$PATH:$NERPA_DEPS/ddlog/bin
            else
                cat >&2 <<EOF
The DDlog environment variables (DDLOG_HOME and PATH) were not set correctly.
You have two options to set necessary environment variables to build Nerpa programs:
1) Run '. scripts/install-nerpa.sh' to install Nerpa dependencies in the expected directory.
2) Manually install DDlog, as per the steps in 'scripts/install-nerpa.sh'.
EOF
            fi
        fi
    else
        # Even without the Nerpa dependencies directory, a Nerpa program can be built if the DDlog environment variables are set correctly.
        if [[ -z $DDLOG_HOME ]]; then
            cat >&2 <<EOF
Nerpa dependencies directory (NERPA_DEPS) was not found in its expected location, and DDlog environment variables are not set correctly.
You have two options to set necessary environment variables to build Nerpa programs:
1) Run '. scripts/install-nerpa.sh' to install Nerpa dependencies in the expected directory.
2) Manually install DDlog, as per the steps in 'scripts/install-nerpa.sh'.
EOF
            exit 1
        fi
    fi
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
rm -f $FILE_NAME.p4info.bin
rm -f $FILE_NAME.p4info.txt
p4c --target bmv2 --arch v1model --p4runtime-files $FILE_NAME.p4info.bin,$FILE_NAME.p4info.txt $FILE_NAME.p4

# Generate DDlog dataplane relations from P4info, using p4info2ddlog.
echo "Generating DDlog relations for dataplane using P4 info..."
cd $NERPA_DIR/p4info2ddlog
cargo run $FILE_DIR $FILE_NAME $NERPA_DIR/dp2ddlog
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
ddlog -i $FILE_NAME.dl

# Optionally, generate necessary code for the management plane.
# Build the OVSDB client crate, which depends on the DDlog crate.
if test -f $FILE_NAME.ovsschema; then
    echo "Building the OVSDB client crate..."

    # Generate a `.toml` for the OVSDB client crate.
    cd $NERPA_DIR
    ./scripts/ovsdb-client-toml.sh $1 $2

    # Generate necessary files for the `ovsdb_client` crate.
    # This crate is built as part of the `nerpa_controller`.
    cd $NERPA_DIR/ovsdb_client
    mkdir -p src/context
    pip3 install -r requirements.txt
    python3 ovsdb2ddlog2rust --schema-file=$FILE_DIR/$FILE_NAME.ovsschema -p nerpa_ --output-file src/context/nerpa_rels.rs
    cd $FILE_DIR
fi

echo "Building controller crate..."
# Substitute the DDlog import name.
sed -i 's/use .*_ddlog/use '$FILE_NAME'_ddlog/' $NERPA_DIR/nerpa_controller/src/nerpa_controller/main.rs

(cd $NERPA_DIR/nerpa_controller && cargo build && cd $NERPA_DIR)
