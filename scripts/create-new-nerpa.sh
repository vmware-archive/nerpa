#!/bin/bash
# Create a new Nerpa program

# Exit when any command fails, since they are all sequential
set -e

if [ "$#" -ne 1 ]; then
    cat >&2 <<EOF
USAGE: $0 PROGRAM_NAME
* PROGRAM_NAME is the name of the Nerpa program, and of the *.p4, *.dl, and *.ovsschema files
EOF
    exit 1
fi

NERPA_DIR=$(pwd)
PROGRAM_NAME=$1

# Make sure the user cannot accidentally override existing files.
set -o noclobber

# Make top-level directory for the program
PROG_DIR=$NERPA_DIR/nerpa_controlplane/$PROGRAM_NAME/
mkdir $PROG_DIR
cd $PROG_DIR

# Create empty P4 program and commands.
touch $PROGRAM_NAME.p4
touch commands.txt

# Create DDlog program.
cat <<EOF > $PROGRAM_NAME.dl 
// Uncomment the following imports after running p4info2ddlog and generating relations from the P4 program.
// import ${PROGRAM_NAME}_dp as ${PROGRAM_NAME}_dp
// import ${PROGRAM_NAME^}_mp as ${PROGRAM_NAME}_mp
EOF

# Create OVSDB schema.
cat <<EOF > $PROGRAM_NAME.ovsschema
{
    "name": "${PROGRAM_NAME}",
    "tables": {},
    "version": "1.7.0"
}
EOF
