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

use proto::p4info::{
    ControllerPacketMetadata,
    Digest,
};
use proto::p4types::P4TypeInfo;

use std::fmt::Write;

/// Writes the Cargo.toml for the dp2ddlog crate.
/// Generated because the DDlog dependency paths depend on the Nerpa program name.
pub fn write_toml(
    io_dir: &str,
    prog_name: &str,
) -> String {
    let ddlog_path = format!("{}/{}_ddlog", io_dir, prog_name);

    format!("
[package]
name = \"dp2ddlog\"
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
types__{}_dp = {{path = \"{}/types/{}_dp\"}}
",
        ddlog_path,
        prog_name,
        ddlog_path,
        ddlog_path,
        prog_name,
        ddlog_path,
        prog_name,
    )
}

/// Writes the dp2ddlog Rust program.
/// Using P4Info, generates code to convert digests and packet metadata to input relations.
pub fn write_rs(
    digests: &[Digest],
    type_info: &P4TypeInfo,
    controller_metadata: &[ControllerPacketMetadata],
    prog_name: &str
) -> Result<String> {
    let mut d2d_out = String::new();
    writeln!(d2d_out, "use proto::p4data::P4Data;")?;
    writeln!(d2d_out, "use byteorder::{{NetworkEndian, ByteOrder}};")?;
    writeln!(d2d_out, "use differential_datalog::program::{{RelId, Update}};")?;
    writeln!(d2d_out, "use differential_datalog::ddval::{{DDValConvert, DDValue}};")?;
    writeln!(d2d_out, "use proto::p4runtime::{{PacketIn, PacketMetadata, PacketOut}};")?;

    writeln!(d2d_out, "use {}_ddlog::Relations;", prog_name)?;
    writeln!(d2d_out, "use {}_ddlog::typedefs::ddlog_std;", prog_name)?;
    writeln!(d2d_out)?;

    // unwrap is safe, because write_digest cannot return an error result
    let digest_out = write_digest(digests, type_info, prog_name).unwrap();
    writeln!(d2d_out, "{}", digest_out)?;

    // unwrap is safe, because write_packet cannot return an error result
    let packetin_out = write_packet(controller_metadata, prog_name, true).unwrap();
    writeln!(d2d_out, "{}", packetin_out)?;

    let packetout_out = write_packet(controller_metadata, prog_name, false).unwrap();
    writeln!(d2d_out, "{}", packetout_out)?;

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

fn write_digest(
    digests: &[Digest],
    type_info: &P4TypeInfo,
    prog_name: &str,
) -> Result<String> {
    let mut d2d_out = String::new();

    writeln!(d2d_out, "pub fn digest_to_ddlog(digest_id: u32, digest_data: &P4Data) -> Option<Update<DDValue>> {{")?;
    if digests.len() == 0 {
        writeln!(d2d_out, "  return None;")?;
        writeln!(d2d_out, "}}")?;
        return Ok(d2d_out);
    }

    // P4 Runtime only allows a digest to be a struct with bitstring fields.
    writeln!(d2d_out, "  let members = digest_data.get_field_struct().get_members();")?;
    writeln!(d2d_out, "  match digest_id {{")?;
    for d in digests.iter() {
        let digest_name = d.get_preamble().get_name();
        let digest_structs = type_info.get_structs().get(digest_name).unwrap();

        writeln!(d2d_out, "    {} => {{", d.get_preamble().get_id())?;

        writeln!(d2d_out, "      Some(Update::Insert {{")?;
        writeln!(d2d_out, "        relid: Relations::{}_dp_{} as RelId,", prog_name, digest_name)?;
        writeln!(d2d_out, "        v: types__{}_dp::{} {{", prog_name, digest_name)?;

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
                    1..=16 => 16,
                    17..=32 => 32,
                    33..=64 => 64,
                    65..=128 => 128,
                    _ => panic!("unsupported bitwidth: {}", bitwidth),
                };

                let handle_u8 = if bitwidth <= 8 {" as u8" } else {""};

                // Get the bitstring, pad it with zeros, and convert it to the correct uint.
                format!("NetworkEndian::read_u{}(&pad_left_zeros(members[{}].get_bitstring(), {})){}", num_bits, mi, num_bits / 8, handle_u8)
            };

            writeln!(d2d_out, "          {}: {},", field_name, field_value)?;
        }

        writeln!(d2d_out, "        }}.into_ddvalue(),")?; // close brace for Update.v
        writeln!(d2d_out, "      }})")?; // close brace for Update
        writeln!(d2d_out, "    }},")?; // close brace for match arm
    }
    writeln!(d2d_out, "    _ => panic!(\"Invalid digest ID: {{}}\", digest_id)")?;

    writeln!(d2d_out, "  }}")?; // close brace for `match`
    writeln!(d2d_out, "}}")?; // close brace for `fn`

    return Ok(d2d_out);
}

fn write_packet(
    controller_metadata: &[ControllerPacketMetadata],
    prog_name: &str,
    is_packet_in: bool,
) -> Result<String> {
    let mut d2d_out = String::new();

    // Handle packet_in and packet_out.
    let (filter, inp_type) = match is_packet_in {
        true => ("packet_in", "PacketIn"),
        false => ("packet_out", "PacketOut")
    };

    writeln!(d2d_out, "pub fn {}_to_ddlog(p: {}) -> Option<Update<DDValue>> {{", filter, inp_type)?;

    // Filter the controller metadata array to the element with name `packet_in`.
    // p4c allows there to be only one header with this name/annotation.
    // If there is a different number of headers, the function outputs None.
    let packet_metadata_vec: Vec<ControllerPacketMetadata> = controller_metadata
        .to_vec()
        .into_iter()
        .filter(|m| m.get_preamble().get_name() == filter)
        .collect();
    if packet_metadata_vec.len() != 1 {
        writeln!(d2d_out, "  return None;")?;
        writeln!(d2d_out, "}}")?;
        return Ok(d2d_out);
    }
    let packet_metadata = &packet_metadata_vec[0];

    writeln!(d2d_out, "  let payload = p.get_payload();")?;
    writeln!(d2d_out, "  let metadata = p.get_metadata().to_vec();")?;
    writeln!(d2d_out, "  Some(Update::Insert{{")?;
    writeln!(d2d_out, "    relid: Relations::{}_dp_{} as RelId,", prog_name, inp_type)?;
    writeln!(d2d_out, "    v: types__{}_dp::{} {{", prog_name, inp_type)?;
    for pm in packet_metadata.get_metadata().iter() {
        let field_name = pm.get_name();

        let id = pm.get_id();
        let field_value = {
            let bitwidth = pm.get_bitwidth();
            let num_bits = match bitwidth {
                1..=16 => 16,
                17..=32 => 32,
                33..=64 => 64,
                65..=128 => 128,
                _ => panic!("unsupported bitwidth: {}", bitwidth),
            };

            let handle_u8 = if bitwidth <= 8 {" as u8" } else {""};

            let meta_value = format!("metadata.iter().filter(|m| m.get_metadata_id() == {}).cloned().collect::<Vec<PacketMetadata>>()[0].get_value()", id);

            let field_value = format!("NetworkEndian::read_u{}(&pad_left_zeros({}, {})){}", num_bits, meta_value, num_bits / 8, handle_u8);

            field_value
        };

        writeln!(d2d_out, "      {}: {},", field_name, field_value)?;
    }
    writeln!(d2d_out, "      packet: ddlog_std::Vec::from(p.get_payload()),")?;
    writeln!(d2d_out, "    }}.into_ddvalue(),")?; // close brace for value
    writeln!(d2d_out, "  }})")?; // close brace for the update
    writeln!(d2d_out, "}}")?; // close brace for `fn`

    return Ok(d2d_out);
}