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
    const P4INFO_ARG: &str = "INPUT.P4INFO.BIN";
    const OUTPUT_ARG: &str = "OUTPUT.DL";
    const RS_OUTPUT_ARG: &str = "OUTPUT.RS";
    const PIPELINE_ARG: &str = "pipeline";
    let matches = App::new("p4info2ddlog")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Outputs DDlog relations corresponding to P4 tables")
        .arg(
            Arg::with_name(P4INFO_ARG)
                .help("binary P4 Runtime file containing the P4 tables")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name(OUTPUT_ARG)
                .help("DDlog output file")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name(RS_OUTPUT_ARG)
                .help(".rs output file for additional generated code")
                .required(false)
                .index(3),
        )
        .arg(
            Arg::with_name(PIPELINE_ARG)
                .help("name of P4 pipeline to convert (all pipelines, by default)")
                .value_name("PIPELINE")
                .takes_value(true)
                .short("p"),
        )
        .get_matches();
    
    p4info_to_ddlog(
        matches.value_of(P4INFO_ARG),
        matches.value_of(OUTPUT_ARG),
        matches.value_of(RS_OUTPUT_ARG),
        matches.value_of(PIPELINE_ARG),
    )
}