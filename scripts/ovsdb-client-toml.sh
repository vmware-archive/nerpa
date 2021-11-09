#!/bin/bash
echo "[package]
name = \"ovsdb_client\"
version = \"0.1.0\"
edition = \"2018\"

[dependencies]
differential_datalog = {path = \"../$1/$2_ddlog/differential_datalog\"}
libc = \"0.2.98\"
ddlog_ovsdb_adapter = {path = \"../$1/$2_ddlog/ovsdb\"}
ovsdb-sys = {path = \"../ovsdb-sys\"}
$2 = {path = \"../$1/$2_ddlog\", features = [\"ovsdb\"]}
memoffset = \"0.6.4\"
serde = \"1.0.126\"
serde_json = \"1.0.65\"" > ovsdb_client/Cargo.toml
