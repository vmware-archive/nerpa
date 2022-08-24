#!/bin/bash
# Script that runs a Nerpa program

# Exit when any command fails, since they are all sequential.
set -e

if test "$1" = "-s"; then
    SIM_IFACES=:
    SUDO=
    TARGET=bmv2
    shift
elif test "$1" = "--ofp4"; then
    SIM_IFACES=:
    SUDO=
    TARGET=ofp4
    shift
else
    SIM_IFACES=false
    TARGET=bmv2
    SUDO=sudo
fi

# Print usage if incorrectly invoked.
if [ "$#" -ne 2 ] || ! [ -d "$1" ]; then
    cat >&2 <<EOF
Usage: $0 [-s | --ofp4] FILE_DIR FILE_NAME
where FILE_DIR contains *.p4, *.dl, and *.ovsschema files
  and FILE_NAME is the name of the *.p4, *.dl, and *.ovsschema files.

Options:
  -s: simulate interfaces over nanomsg instead of veth devices
  --ofp4: use OVS and ofp4 instead of bmv2
EOF

    exit 1
fi

# Check if the Nerpa dependencies were installed correctly.
# '$NERPA_DEPS' should point to a directory containing a 'behavioral-model' subdirectory.
if [[ -z $NERPA_DEPS ]]; then
    NERPA_DEPS=$(pwd)/nerpa-deps

    # If the Nerpa dependencies directory exists as expected, set the environment variables.
    if [ -d $NERPA_DEPS ] && [ -d $NERPA_DEPS/behavioral-model ]; then
        export NERPA_DEPS
    else
        cat >&2 <<EOF
Nerpa dependencies directory (NERPA_DEPS) was not found in its expected location.
You have two options to set necessary environment variables to build nerpa programs.
1) Run '. install-nerpa.sh' to install Nerpa dependencies in the expected directory.
2) Manually execute the steps in 'scripts/install-nerpa.sh' in the desired locations.
EOF
        exit 1
    fi
fi

echo "Running a Nerpa program..."

NERPA_DIR=$(pwd)
FILE_DIR=$NERPA_DIR/$1
FILE_NAME=$2

# We will do some work with OVS later if we use it as our switch or if
# we set up an OVSDB instance.  Set up a sandbox directory and tell
# OVS to use it.  By doing this, we can avoid messing up any system
# OVS configuration.
rm -rf sandbox
mkdir sandbox
sandbox=$(cd sandbox && pwd)
env_sh=$sandbox/env.sh
cat >$env_sh <<EOF
OVS_RUNDIR='$sandbox'; export OVS_RUNDIR
OVS_LOGDIR='$sandbox'; export OVS_LOGDIR
OVS_DBDIR='$sandbox'; export OVS_DBDIR
OVS_SYSCONFDIR='$sandbox'; export OVS_SYSCONFDIR
EOF
. "$env_sh"

# Ensure that any OVS-related daemons get cleaned up after we exit.
> "$sandbox"/dummy.pid          # Avoid error if we fail before creating any other pidfiles.
trap 'kill `cat "$sandbox"/*.pid`' 0 1 2 3 13 14 15

# Set up the virtual switch.
case $TARGET in
    bmv2)
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
        ;;

    ofp4)
        schema=$NERPA_DIR/ovs/ovs/vswitchd/vswitch.ovsschema
        if test ! -e "$schema"; then
            echo >&2 "$0: Open vSwitch configuration schema not found in expected location ($schema)"
            exit 1
        fi

        # Create an OVS configuration database and start the database server.
        pushd "$sandbox" >/dev/null
        >> .conf.db.~lock~
        ovsdb-tool create conf.db "$schema"
        ovsdb-server --detach --no-chdir --pidfile -vconsole:off --log-file -vsyslog:off \
                     --remote=punix:"$sandbox"/db.sock \
                     --remote=db:Open_vSwitch,Open_vSwitch,manager_options
        ovs-vsctl --no-wait -- init

        # Start OVS, and add a bridge and some ports.
        echo "Starting sandboxed OVS..."
        ovs-vswitchd --detach --no-chdir --pidfile -vconsole:off --log-file -vsyslog:off \
                     --enable-dummy=override -vvconn -vnetdev_dummy
        ovs-vsctl add-br br0 -- add-port br0 p0 -- add-port br0 p1 -- add-port br0 p2 -- add-port br0 p3
        echo "To use OVS from the perspective of the sandbox, set OVS_* variables in your shell:"
        echo "    . '$env_sh'"

        popd >/dev/null

        # Start ofp4.
        echo "Starting ofp4..."
        (cd ofp4 && cargo build)
        ofp4/target/debug/ofp4 $FILE_NAME unix:"$sandbox"/br0.mgmt &
        echo $! > "$sandbox"/ofp4.pid
        ;;
esac

# Optionally, start OVSDB.
SCHEMA=$FILE_DIR/$FILE_NAME.ovsschema
if test -f "$SCHEMA"; then
    pushd "$sandbox" >/dev/null

    echo "Creating $FILE_NAME database..."
    db=$FILE_NAME.db
    ovsdb-tool create "$db" "$SCHEMA"

    echo "Starting OVSDB..."
    ovsdb-server --pidfile=nerpa-ovsdb-server.pid --detach --log-file --remote=punix:nerpa.sock --remote=ptcp:6640 "$db"

    popd >/dev/null
fi

# If a script with an initial OVSDB command was provided, execute that script in the background.
INIT_SCRIPT=$FILE_DIR/init-ovsdb.sh
if test -f "$INIT_SCRIPT"; then
    echo "Initializing OVSDB contents in background..."
    $INIT_SCRIPT &
fi

# Run the controller.
(cd $NERPA_DIR/nerpa_controller && RUST_BACKTRACE=FULL cargo run -- --ddlog-record=replay.txt $FILE_DIR $FILE_NAME && cd $NERPA_DIR)
