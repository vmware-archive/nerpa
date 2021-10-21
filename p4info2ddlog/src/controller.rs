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
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::io::Write as IoWrite;

pub fn write_toml(
    io_dir: String,
    prog_name: String,
    digest_path_opt: Option<&str>,
) -> Result<()> {
    let toml_fn = "../nerpa_controller/Cargo.toml";
    let file = File::open(toml_fn)?;
    let reader = BufReader::new(file);

    let mut toml_out = String::new();

    let prog_str = prog_name.clone();

    let reserved_keys: HashSet<&str> = [
        "differential_datalog",
        "digest2ddlog",
        "types",
        prog_str.as_str(),
    ].iter().cloned().collect();

    for line_res in reader.lines() {
        let line = line_res?;

        let token_opt = line.split_whitespace().next();

        // Preserve whitespace.
        if token_opt.is_none() {
            writeln!(toml_out)?;
            continue;
        }

        // Skip the lines with reserved inputs.
        if reserved_keys.contains(token_opt.unwrap()) {
            // writeln!("matched reserved keyword: {}", token_opt.unwrap());
            continue;
        }

        // Print all other lines.
        writeln!(toml_out, "{}", line)?;
    }

    // Write the dependencies that vary based on the user input.
    writeln!(toml_out, "differential_datalog = {{path = \"{}/{}_ddlog/differential_datalog\"}}", io_dir, prog_name)?;
    writeln!(toml_out, "{} = {{path = \"{}/{}_ddlog\"}}", prog_name, io_dir, prog_name)?;
    writeln!(toml_out, "types = {{path = \"{}/{}_ddlog/types\"}}", io_dir, prog_name)?;

    if !digest_path_opt.is_none() {
        writeln!(toml_out, "digest2ddlog = {{path = \"{}\"}}", digest_path_opt.unwrap())?;
    }

    let toml_fn_os = OsStr::new(&toml_fn);
    File::create(toml_fn_os)
        .with_context(|| format!("{}: create failed", toml_fn))?
        .write_all(toml_out.as_bytes())
        .with_context(|| format!("{}: write failed", toml_fn))?;

    Ok(())
}