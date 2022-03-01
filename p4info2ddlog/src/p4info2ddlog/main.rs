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

use anyhow::Result;

use clap::{App, Arg};

use p4info2ddlog::p4info_to_ddlog;

use std::env;

fn main() -> Result<()> {
    const FILE_DIR_ARG: &str = "FILE_DIR";
    const FILE_NAME_ARG: &str = "FILE_NAME";
    const CRATE_ARG: &str = "OUTPUT_CRATE_DIR";
    const PIPELINE_ARG: &str = "pipeline";

    let matches = App::new("p4info2ddlog")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Outputs DDlog relations corresponding to P4 tables")
        .arg(
            Arg::with_name(FILE_DIR_ARG)
                .help("path to directory with input file (*.p4info.bin) and where output (*.dl) will be written")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name(FILE_NAME_ARG)
                .help("program name before the extension: {program}.p4info.bin, {program}.dl")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name(CRATE_ARG)
                .help("path to directory for dp2ddlog helper crate (optional)")
                .required(false)
                .index(3),
        )
        .arg(
            Arg::with_name(PIPELINE_ARG)
                .help("name of P4 pipeline to convert (all pipelines, by default")
                .value_name("PIPELINE")
                .takes_value(true)
                .short("p"),
        )
        .get_matches();
    
    p4info_to_ddlog(
        matches.value_of(FILE_DIR_ARG),
        matches.value_of(FILE_NAME_ARG),
        matches.value_of(CRATE_ARG),
        matches.value_of(PIPELINE_ARG),
    )
}
