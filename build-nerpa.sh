#!/bin/bash
# Script that runs a Nerpa program

echo "Running a Nerpa program..."

if [ "$#" -ne 2 ] || ! [ -d "$1" ]; then
    echo "USAGE: $0 FILE_DIR FILE_NAME" >&2
    echo "* FILE_DIR: directory containing *.p4, *.dl, and optional *.ovsschema files"
    echo "* FILE_NAME: name of the *p4, *dl, and *ovsschema files"
    exit 1
fi

export NERPA_DIR=$(pwd)
export FILE_DIR=$NERPA_DIR/$1
export FILE_NAME=$2

# TODO: Once the install script is done, remove setting $DDLOG_HOME.
cd ../../nerpa-deps/ddlog
export DDLOG_HOME=$(pwd)
echo $DDLOG_HOME

export PATH=$PATH:$DDLOG_HOME/bin

if test ! -f "$DDLOG_HOME/lib/ddlog_std.dl"; then
    echo >&2 "$0: \$DDLOG_HOME must point to the ddlog tree"
    exit 1
fi

cd $FILE_DIR

# Optionally, generate DDlog input relations from the OVSDB schema.
if test -f $FILE_NAME.ovsschema; then
    echo "Generating DDlog input relations from OVSDB schema..."
    ovsdb2ddlog -f $FILE_NAME.ovsschema --output-file=$2_mp.dl
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
if test ! -f "$FILE_NAME.dl"; then
    echo >&2 "$0: could not find DDlog program $FILE_NAME.dl in $1"
    exit 1
fi

echo "Compiling DDlog crate..."
ddlog -i $FILE_NAME.dl &&
(cd ${FILE_NAME}_ddlog && cargo build --release && cd ..)

# Set the nerpa dependencies directory.
# TODO: Move the nerpa-deps to inside `nerpa` for self-containment.
# TODO: Handle this in the installation script.
NERPA_DEPS_REL_PATH=../../nerpa-deps

cd $NERPA_DIR
if test ! -d $NERPA_DEPS_REL_PATH; then
    echo >&2 "$0: did not find nerpa-deps in expected location"
    exit 1
fi
cd $NERPA_DEPS_REL_PATH 
export NERPA_DEPS=$(pwd)

# Kill any running `simple_switch_grpc` processes.
echo "Resetting network configs..."
sudo pkill -f simple_switch_grpc

# Tear down any existing virtual eth interfaces.
for idx in 0 1 2 3; do
    intf="veth$(($idx*2))"
    if sudo ip link show $intf &> /dev/null; then
        sudo ip link delete $intf type veth
    fi
done

# Set up the virtual eth interfaces.
for idx in 0 1 2 3; do
    intf0="veth$(($idx*2))"
    intf1="veth$(($idx*2 + 1))"
    if ! sudo ip link show $intf0 &> /dev/null; then
        sudo ip link add name $intf0 type veth peer name $intf1
        sudo ip link set dev $intf0 up
        sudo ip link set dev $intf1 up

        sudo sysctl net.ipv6.conf.${intf0}.disable_ipv6=1
        sudo sysctl net.ipv6.conf.${intf1}.disable_ipv6=1
    fi
done

# Run the simple switch GRPC.
export SWITCH_EMULATOR_PATH=$NERPA_DEPS/behavioral-model
export SWITCH_GRPC_EXEC=$SWITCH_EMULATOR_PATH/targets/simple_switch_grpc/simple_switch_grpc
if test ! -f $SWITCH_GRPC_EXEC; then
    echo >&2 "$0: did not find simple-switch-grpc executable in expected location ($SWITCH_GRPC_EXEC)"
    exit 1
fi

export SWITCH_SETTINGS=$FILE_DIR/$FILE_NAME.json
if test ! -f $SWITCH_SETTINGS; then
    echo >&2 "$0: did not find settings JSON file in expected location ($SWITCH_SETTINGS)"
    exit 1
fi

export SWITCH_FLAGS="--log-console -i 0@veth1 -i 1@veth3 -i 2@veth5 -i 3@veth7 $SWITCH_SETTINGS"
export GRPC_FLAGS="--grpc-server-addr 0.0.0.0:50051 --cpu-port 1010"

sudo $SWITCH_GRPC_EXEC $SWITCH_FLAGS -- $GRPC_FLAGS & sleep 2 

# Configure the switch.
export COMMANDS_FILE=$FILE_DIR/commands.txt
if test ! -f $COMMANDS_FILE; then
    echo >&2 "$0: did not find simple switch config commands in expected location ($COMMANDS_FILE)"
    exit 1
fi

export TOOLS_PATH=$SWITCH_EMULATOR_PATH/tools/
export CLI_EXEC=$SWITCH_EMULATOR_PATH/targets/simple_switch/sswitch_CLI.py
chmod +x $CLI_EXEC

PYTHONPATH=$TOOLS_PATH python3 $CLI_EXEC < $COMMANDS_FILE

# Build and run the controller.
cd $NERPA_DIR/nerpa_controller
cargo run
