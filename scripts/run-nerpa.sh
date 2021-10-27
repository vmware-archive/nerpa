#!/bin/bash
# Script that runs a Nerpa program

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
    echo "Run `. install-nerpa.sh` to set these variables."
    exit 1
fi

if test ! -d $NERPA_DEPS; then
    echo >&2 "$0: could not find `nerpa-deps` in expected location ($NERPA_DEPS)"
    exit 1
fi

echo "Running a Nerpa program..."

export NERPA_DIR=$(pwd)
export FILE_DIR=$NERPA_DIR/$1
export FILE_NAME=$2

# Set up the virtual switch.

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
    sudo pkill -f simple_switch_grpc
    exit 1
fi

export TOOLS_PATH=$SWITCH_EMULATOR_PATH/tools/
export CLI_EXEC=$SWITCH_EMULATOR_PATH/targets/simple_switch/sswitch_CLI.py
chmod +x $CLI_EXEC

PYTHONPATH=$TOOLS_PATH python3 $CLI_EXEC < $COMMANDS_FILE

# Run the controller.
(cd $NERPA_DIR/nerpa_controller && cargo run $FILE_DIR $FILE_NAME && cd $NERPA_DIR)
