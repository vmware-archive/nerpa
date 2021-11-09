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

use proto::p4info::Digest;
use proto::p4types::P4TypeInfo;

use std::fmt::Write;

pub fn write_rs(
    digests: &[Digest],
    type_info: &P4TypeInfo,
    prog_name: &str
) -> Result<String> {
    let mut d2d_out = String::new();
    writeln!(d2d_out, "use proto::p4data::P4Data;")?;
    writeln!(d2d_out, "use byteorder::{{NetworkEndian, ByteOrder}};")?;
    writeln!(d2d_out, "use differential_datalog::program::{{RelId, Update}};")?;
    writeln!(d2d_out, "use differential_datalog::ddval::{{DDValConvert, DDValue}};")?;

    writeln!(d2d_out, "use {}_ddlog::Relations;", prog_name)?;
    writeln!(d2d_out)?;
    writeln!(d2d_out, "pub fn digest_to_ddlog(digest_id: u32, digest_data: &P4Data) -> Update<DDValue> {{")?;

    // This function works because P4 Runtime only allows a digest to be a struct with bitstring fields.
    writeln!(d2d_out, "  let members = digest_data.get_field_struct().get_members();")?;
    writeln!(d2d_out, "  match digest_id {{")?;

    for d in digests.iter() {
        let digest_name = d.get_preamble().get_name();
        let digest_structs = type_info.get_structs().get(digest_name).unwrap();

        writeln!(d2d_out, "    {} => {{", d.get_preamble().get_id())?;

        writeln!(d2d_out, "      Update::Insert {{")?;
        writeln!(d2d_out, "        relid: Relations::{} as RelId,", digest_name)?;
        writeln!(d2d_out, "        v: types::{} {{", digest_name)?;

        // Write Update value fields using digest struct members.
        for (mi, m) in digest_structs.get_members().iter().enumerate() {
            let member_type_spec = m.get_type_spec();

            if !member_type_spec.has_bitstring() || !member_type_spec.get_bitstring().has_bit() {
                panic!("digest struct fields can only have bitstrings of type bit");
            }

            let field_name = m.get_name();

            let field_value = {
                let bitwidth = member_type_spec.get_bitstring().get_bit().get_bitwidth();

                let num_bits = match bitwidth {
                    1..=8 => 8,
                    9..=16 => 16,
                    17..=32 => 32,
                    33..=64 => 64,
                    65..=128 => 128,
                    _ => panic!("unsupported bitwidth: {}", bitwidth),
                };

                // Get the bitstring, pad it with zeros, and convert it to the correct uint.
                format!("NetworkEndian::read_u{}(&pad_left_zeros(members[{}].get_bitstring(), {}))", num_bits, mi, num_bits / 8)
            };

            writeln!(d2d_out, "          {}: {},", field_name, field_value)?;
        }

        writeln!(d2d_out, "        }}.into_ddvalue(),")?; // close brace for Update.v
        writeln!(d2d_out, "      }}")?; // close brace for Update
        writeln!(d2d_out, "    }},")?; // close brace for match arm
    }
    writeln!(d2d_out, "    _ => panic!(\"Invalid digest ID: {{}}\", digest_id)")?;

    writeln!(d2d_out, "  }}")?; // close brace for `match`
    writeln!(d2d_out, "}}")?; // close brace for `fn`
    writeln!(d2d_out)?;

    let helpers = "
fn pad_left_zeros(inp: &[u8], size: usize) -> Vec<u8> {
    if inp.len() > size {
        panic!(\"input buffer exceeded provided length\");
    }

    let mut buf = vec![0; size];
    let offset = size - inp.len();
    for i in 0..inp.len() {
        buf[i + offset] = inp[i];
    }

    buf
}";
    writeln!(d2d_out, "{}", helpers)?;

    Ok(d2d_out)
}

pub fn write_toml(
    io_dir: &str,
    prog_name: &str,
) -> String {
    let ddlog_path = format!("{}/{}_ddlog", io_dir, prog_name);

    format!("
[package]
name = \"digest2ddlog\"
version = \"0.1.0\"
authors = [\"Debnil Sur <dsur@vmware.com>\"]
edition = \"2018\"

[lib]
path = \"src/lib.rs\"

[dependencies]
byteorder = \"1.4.3\"
differential_datalog = {{path = \"{}/differential_datalog\"}}
{} = {{path = \"{}\"}}
proto = {{path = \"../proto\"}}
types = {{path = \"{}/types\"}}
",
        ddlog_path,
        prog_name,
        ddlog_path,
        ddlog_path,
    )
}
