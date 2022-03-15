/*
Copyright (c) 2021 VMware, Inc.
SPDX-License-Identifier: MIT
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt::Write;
use std::fs::{
    File,
    metadata
};
use std::io::{BufRead, BufReader};
use std::io::Write as IoWrite;
use std::path::Path;

const TOML_FN: &str = "../nerpa_controller/Cargo.toml";

/// Write the TOML for the nerpa controller.
///
/// # Arguments
/// * `io_dir` - filepath to directory with P4 and DDlog files.
/// * `prog_name` - name of the Nerpa program.
/// * `dp_path_opt` - filepath to the dp2ddlog crate, if provided.
pub fn write_toml(
    io_dir: &str,
    prog_name: &str,
    dp_path_opt: Option<&str>,
) -> Result<()> {
    let types_dp_name = format!("types__{}_dp", prog_name);
    let reserved_keys: HashSet<&str> = [
        "differential_datalog",
        "dp2ddlog",
        "types",
        "ovsdb_client",
        types_dp_name.as_str(),
        prog_name,
    ].iter().cloned().collect();

    let mut toml_out = match Path::new(TOML_FN).exists() {
        true => edit_toml(reserved_keys)?,
        false => create_toml(),
    };

    // Write the dependencies that vary based on the user input.
    writeln!(toml_out, "differential_datalog = {{path = \"{}/{}_ddlog/differential_datalog\"}}", io_dir, prog_name)?;
    writeln!(toml_out, "{} = {{path = \"{}/{}_ddlog\"}}", prog_name, io_dir, prog_name)?;
    writeln!(toml_out, "types = {{path = \"{}/{}_ddlog/types\"}}", io_dir, prog_name)?;
    writeln!(toml_out, "types__{}_dp = {{path = \"{}/{}_ddlog/types/{}_dp\"}}", prog_name, io_dir, prog_name, prog_name)?;

    if !dp_path_opt.is_none() {
        writeln!(toml_out, "dp2ddlog = {{path = \"{}\"}}", dp_path_opt.unwrap())?;
    }

    // If the program directory contains an OVS schemafile, we add the ovsdb client dependency.
    let ovs_schema_fn = format!("{}/{}.ovsschema", io_dir, prog_name);
    if metadata(ovs_schema_fn.as_str()).is_ok() {
        writeln!(toml_out, "ovsdb_client = {{path = \"../ovsdb_client\"}}")?;
    }

    let toml_fn_os = OsStr::new(&TOML_FN);
    File::create(toml_fn_os)
        .with_context(|| format!("{}: create failed", TOML_FN))?
        .write_all(toml_out.as_bytes())
        .with_context(|| format!("{}: write failed", TOML_FN))?;

    Ok(())
}

fn edit_toml(
    reserved_keys: HashSet<&str>,
) -> Result<String> {
    let toml_fn = "../nerpa_controller/Cargo.toml";
    let file = File::open(toml_fn)?;
    let reader = BufReader::new(file);

    let mut toml_out = String::new();

    for line_res in reader.lines() {
        let line = line_res?;

        // Check the first token.
        let token_opt = line.split_whitespace().next();

        // Preserve whitespace.
        if token_opt.is_none() {
            writeln!(toml_out)?;
            continue;
        }

        // Skip the lines with reserved inputs.
        if reserved_keys.contains(token_opt.unwrap()) {
            continue;
        }

        // Exclude any dependences that include `nerpa_controlplane`.
        // Since Nerpa programs are written in this subdirectory, that should remove
        // any additional dependencies associated with old programs.
        if line.contains("nerpa_controlplane") {
            continue;
        }

        // Print all other lines.
        writeln!(toml_out, "{}", line)?;
    }

    Ok(toml_out)
}

fn create_toml() -> String {
    format!(
"[package]
name = \"nerpa_controller\"
version = \"0.1.0\"
authors = [\"Debnil Sur <dsur@vmware.com>\"]
edition = \"2018\"

[lib]
path = \"src/lib.rs\"

[[bin]]
name = \"nerpa-controller\"
path = \"src/nerpa_controller/main.rs\"
doc = false

[build-dependencies]
protoc-grpcio = \"3.0.0\"

[dependencies]
clap = \"2.33.3\"
futures = \"0.3.12\"
grpcio = \"0.9.0\"
itertools = \"0.10.0\"
num-traits = \"0.2.14\"
p4ext = {{path = \"../p4ext\"}}
proto = {{path = \"../proto\"}}
protobuf = \"2.22.0\"
protobuf-codegen = \"2.22.0\"
tokio = {{ version = \"1.2.0\", features = [\"full\"]}}
tracing = \"0.1\"
"
    )
}
