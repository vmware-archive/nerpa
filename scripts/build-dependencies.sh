#! /bin/sh -ex

usage() {
    cat <<EOF
usage: $0 DIRECTORY
where DIRECTORY is the name of a directory that does not yet exist
to create the build in.
EOF
}
if test "$#" != 1; then
    usage
    exit 1
elif test "$1" = "--help"; then
    usage
    exit 0
elif test -e "$1"; then
    echo 2>&1 "$0: $1 already exists, please specify the name of a directory that does not yet exist"
    exit 1
fi

mkdir "$1"
cd "$1"

NERPA_DEPS=$(pwd)
cat > envvars.sh <<EOF
export NERPA_DEPS='$NERPA_DEPS'
export DDLOG_HOME=\$NERPA_DEPS/ddlog-v1.2.3
export PATH=\$DDLOG_HOME/bin:\$NERPA_DEPS/inst/bin:\$NERPA_DEPS/inst/sbin:\$PATH
export LD_LIBRARY_PATH=\$NERPA_DEPS/inst/lib
EOF

. ./envvars.sh

wget https://github.com/vmware/differential-datalog/releases/download/v1.2.3/ddlog-v1.2.3-20211213235218-Linux.tar.gz
tar xzf ddlog-v1.2.3-20211213235218-Linux.tar.gz
mv ddlog ddlog-v1.2.3

CONFIGURE="./configure --prefix=$NERPA_DEPS/inst CPPFLAGS=-I$NERPA_DEPS/inst/include LDFLAGS=-L$NERPA_DEPS/inst/lib"

git clone --recursive https://github.com/p4lang/PI.git
(cd PI && ./autogen.sh && $CONFIGURE --with-proto --without-internal-rpc --without-cli --without-bmv2)
(cd PI && make -j$(nproc) && make install)

git clone https://github.com/p4lang/behavioral-model.git
(cd behavioral-model && ./autogen.sh && $CONFIGURE --with-pi)
(cd behavioral-model && make -j$(nproc) install)
(cd behavioral-model/targets/simple_switch_grpc/ && $CONFIGURE --with-thrift && make -j$(nproc) install)

git clone --recursive https://github.com/p4lang/p4c.git
mkdir p4c/build
(cd p4c/build && cmake -DCMAKE_INSTALL_PREFIX:PATH=$NERPA_DEPS/inst .. && make -j$(nproc) && make install)

git clone --recursive git@github.com:vmware/nerpa.git
(cd nerpa/ovs/ovs && ./boot.sh && $CONFIGURE --enable-shared && make -j$(nproc) && make install)