#!/bin/bash
# Script that runs a Nerpa program

# Exit when any command fails, since they are all sequential.
set -e

if test "$1" = "-s"; then
    SIM_IFACES=:
    SUDO=
    shift
else
    SIM_IFACES=false
    SUDO=sudo
fi

# Print usage if incorrectly invoked.
if [ "$#" -ne 2 ] || ! [ -d "$1" ]; then
    cat >&2 <<EOF
USAGE: $0 [-s] FILE_DIR FILE_NAME
where FILE_DIR contains *.p4, *.dl, and optional *.ovsschema files
  and FILE_NAME is the name of the *p4, *dl, and *ovsschema files.

Options:
  -s: simulate interfaces over nanomsg instead of veth devices
EOF

    exit 1
fi

# Check if the Nerpa dependencies were installed correctly.
if [[ -z $NERPA_DEPS ]]; then
    NERPA_DEPS=$(pwd)/nerpa-deps

    # If the Nerpa dependencies directory exists, set the environment variables.
    if [ -d $NERPA_DEPS ]; then
        export NERPA_DEPS
    else
        echo "Nerpa dependencies directory (NERPA_DEPS) was not found in its expected location."

        echo "You have two options to set necessary environment variables to build nerpa programs."
        echo "1) Run '. install-nerpa.sh' to install Nerpa dependencies in the expected directory."
        echo "2) Manually execute the steps in 'scripts/install-nerpa.sh' in the desired locations."
        exit 1
    fi
fi

echo "Running a Nerpa program..."

NERPA_DIR=$(pwd)
FILE_DIR=$NERPA_DIR/$1
FILE_NAME=$2

# Set up the virtual switch.

# Kill any running `simple_switch_grpc` processes.
echo "Resetting network configs..."
$SUDO pkill -f simple_switch_grpc || :

if ! $SIM_IFACES; then
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
fi

# Run the simple switch GRPC.
SWITCH_EMULATOR_PATH=$NERPA_DEPS/behavioral-model
SWITCH_GRPC_EXEC=$SWITCH_EMULATOR_PATH/targets/simple_switch_grpc/simple_switch_grpc
if test ! -f $SWITCH_GRPC_EXEC; then
    echo >&2 "$0: did not find simple-switch-grpc executable in expected location ($SWITCH_GRPC_EXEC)"
    exit 1
fi

SWITCH_SETTINGS=$FILE_DIR/$FILE_NAME.json
if test ! -f $SWITCH_SETTINGS; then
    echo >&2 "$0: did not find settings JSON file in expected location ($SWITCH_SETTINGS)"
    exit 1
fi

SWITCH_FLAGS="--log-console $SWITCH_SETTINGS"
if $SIM_IFACES; then
    SWITCH_FLAGS+=" --packet-in ipc://bmv2.ipc"
else
    SWITCH_FLAGS+=" -i 0@veth1 -i 1@veth3 -i 2@veth5 -i 3@veth7"
fi
GRPC_FLAGS="--grpc-server-addr 0.0.0.0:50051 --cpu-port 510"

$SUDO $SWITCH_GRPC_EXEC $SWITCH_FLAGS -- $GRPC_FLAGS & sleep 2 

# Configure the switch.
COMMANDS_FILE=$FILE_DIR/commands.txt
if test ! -f $COMMANDS_FILE; then
    echo >&2 "$0: did not find simple switch config commands in expected location ($COMMANDS_FILE)"
    $SUDO pkill -f simple_switch_grpc
    exit 1
fi

TOOLS_PATH=$SWITCH_EMULATOR_PATH/tools/
CLI_EXEC=$SWITCH_EMULATOR_PATH/targets/simple_switch/sswitch_CLI.py
chmod +x $CLI_EXEC

sed '/^#/d' $COMMANDS_FILE | PYTHONPATH=$TOOLS_PATH python3 $CLI_EXEC

# Optionally, start OVSDB.
SCHEMA=$FILE_DIR/$FILE_NAME.ovsschema
if test -f "$SCHEMA"; then
    # Use the current directory instead of the system ones, for testing purposes.
    export OVS_LOGDIR=$(pwd)
    export OVS_RUNDIR=$(pwd)
    export OVS_SYSCONFDIR=$(pwd)
    export OVS_PKGDATADIR=$(pwd)

    echo "Stopping OVSDB..."
    ovs-appctl -t ovsdb-server exit || :

    echo "Creating database..."
    DB=$FILE_NAME.db
    rm -f "$DB"
    ovsdb-tool create "$DB" "$SCHEMA"

    echo "Starting OVSDB..."
    ovsdb-server --pidfile --detach --log-file --remote=punix:db.sock --remote=ptcp:6640 "$DB"
fi

# Run the controller.
(cd $NERPA_DIR/nerpa_controller && RUST_BACKTRACE=FULL cargo run -- --ddlog-record=replay.txt $FILE_DIR $FILE_NAME && cd $NERPA_DIR)
