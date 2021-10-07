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

use anyhow::{anyhow, Context, Result};

use multimap::MultiMap;

use proto::p4info::{self, Action, P4Info, Table};

use protobuf::{Message, RepeatedField};

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write;
use std::fs::File;
use std::io::Write as IoWrite;

type Annotations = RepeatedField<String>;

trait Annotation {
    fn has_annotation(&self, name: &str) -> bool;
}

impl Annotation for Annotations {
    fn has_annotation(&self, field_name: &str) -> bool {
        self.iter().any(|e| e == field_name)
    }
}

// Returns the ddlog type to use for a 'bitwidth'-bit P4 value
// annotated with 'annotations'.
fn p4_basic_type(bitwidth: i32, annotations: &Annotations) -> String {
    if bitwidth == 1 && annotations.has_annotation("@nerpa_bool") {
        "bool".into()
    } else {
        format!("bit<{}>", bitwidth)
    }
}

fn read_p4info(filename_os: &OsStr) -> Result<P4Info> {
    let filename = filename_os.to_string_lossy();
    let mut file = File::open(filename_os).with_context(|| format!("{}: open failed", filename))?;
    Message::parse_from_reader(&mut file).with_context(|| format!("{}: read failed", filename))
}

fn get_pipelines(
    p4info: P4Info,
    pipeline_arg: Option<&str>,
) -> Result<MultiMap<String, Table>> {
    // Break up table names into "<pipeline>.<table>" and group by pipeline.
    let mut pipelines: MultiMap<String, Table> = p4info
        .get_tables()
        .iter()
        .cloned()
        .filter_map(|table| {
            match table
                .get_preamble()
                .name
                .split('.')
                .collect::<Vec<_>>()
                .as_slice()
            {
                [pipeline_name, table_name] => {
                    let mut table = table.clone();
                    table.mut_preamble().set_name(table_name.to_string());
                    Some((pipeline_name.to_string(), table))
                }
                _ => None,
            }
        })
        .collect();

    if pipelines.is_empty() {
        return Err(anyhow!("P4Info has no pipelines"));
    }

    // If a pipeline argument was specified, keep only the specified pipeline.
    if let Some(pipeline_name) = pipeline_arg {
        if !pipelines.contains_key(pipeline_name) {
            return Err(anyhow!("P4Info has no pipeline {}", pipeline_name));
        }
        pipelines.retain(|k, _| k == pipeline_name);
    };

    Ok(pipelines)
}


use proto::p4types::P4DataTypeSpec_oneof_type_spec as P4DataTypeSpec;

fn extract_p4data_types(
    type_spec: &Option<proto::p4types::P4DataTypeSpec_oneof_type_spec>
) -> Vec<String> {
    let mut types = Vec::<String>::new();
    match type_spec {
        Some(P4DataTypeSpec::bitstring(_)) => {},
        Some(P4DataTypeSpec::bool(_)) => {},
        Some(P4DataTypeSpec::tuple(t)) => {
            for tm in t.get_members().iter() {
                types.append(&mut extract_p4data_types(&tm.type_spec));
            }
        },
        Some(P4DataTypeSpec::field_struct(ref fs)) => types.push(fs.get_name().to_owned()),
        Some(P4DataTypeSpec::header(ref h)) => types.push(h.get_name().to_owned()),
        Some(P4DataTypeSpec::header_union(ref hu)) => types.push(hu.get_name().to_owned()),
        Some(P4DataTypeSpec::header_stack(ref hs)) => types.push(hs.get_header().get_name().to_owned()),
        Some(P4DataTypeSpec::header_union_stack(ref hus)) => types.push(hus.get_header_union().get_name().to_owned()),
        Some(P4DataTypeSpec::field_enum(ref fe)) => types.push(fe.get_name().to_owned()),
        Some(P4DataTypeSpec::error(ref e)) => types.push(format!("error")), // TODO: Find a cleaner method for errors.
        Some(P4DataTypeSpec::serializable_enum(ref se)) => types.push(se.get_name().to_owned()),
        Some(P4DataTypeSpec::new_type(ref nt)) => types.push(nt.get_name().to_owned()),
        None => {},
    };

    types
}

fn p4data_to_ddlog_type(
    type_spec: &Option<proto::p4types::P4DataTypeSpec_oneof_type_spec>
) -> String {
    use proto::p4types::P4BitstringLikeTypeSpec_oneof_type_spec as P4BitstringTypeSpec;

    match type_spec {
        Some(P4DataTypeSpec::bitstring(ref bs)) => {
            let bitwidth: i32 = match &bs.type_spec {
                Some(P4BitstringTypeSpec::bit(b)) => b.get_bitwidth(),
                Some(P4BitstringTypeSpec::int(i)) => i.get_bitwidth(),
                Some(P4BitstringTypeSpec::varbit(v)) => v.get_max_bitwidth(),
                None => 0, // should never happen
            };

            format!("bit<{}>", bitwidth)
        },
        Some(P4DataTypeSpec::bool(_)) => format!("bool"),
        Some(P4DataTypeSpec::tuple(t)) => {
            let mut tuple_types = Vec::new();
            for tm in t.get_members().iter() {
                tuple_types.push(p4data_to_ddlog_type(&tm.type_spec));
            }

            format!("({})",  tuple_types.join(","))
        },

        // P4NamedType contains the name of a P4 type.
        // For all enum variants of this type, their corresponding DDlog type is just that named type.
        Some(P4DataTypeSpec::field_struct(ref fs)) => fs.get_name().to_owned(),
        Some(P4DataTypeSpec::header(ref h)) => h.get_name().to_owned(),
        Some(P4DataTypeSpec::header_union(ref hu)) => hu.get_name().to_owned(),

        // P4HeaderStackTypeSpec is a (header, size) pair.
        // `header` is a named P4 type, size is an int32.
        Some(P4DataTypeSpec::header_stack(ref hs)) => format!("({}, bigint)", hs.get_header().get_name()),

        // P4HeaderUnionStackTypeSpec consists of a (header_union, size) pair.
        // `header_union` is a named type, size is an int32.
        Some(P4DataTypeSpec::header_union_stack(ref hus)) => format!("({}, bigint)", hus.get_header_union().get_name()),
        Some(P4DataTypeSpec::field_enum(ref fe)) => fe.get_name().to_owned(),

        // TODO: Create P4 error type in DDlog.
        Some(P4DataTypeSpec::error(ref _e)) => format!("error"),
        Some(P4DataTypeSpec::serializable_enum(ref se)) => se.get_name().to_owned(),
        Some(P4DataTypeSpec::new_type(ref nt)) => nt.get_name().to_owned(),
        None => format!(""), // should never happen
    }
}

pub fn p4info_to_ddlog(
    p4info_arg: Option<&str>,
    output_arg: Option<&str>,
    rs_output_arg: Option<&str>,
    pipeline_arg: Option<&str>,
) -> Result<()> {
    let p4info = read_p4info(OsStr::new(&p4info_arg.unwrap().clone()))?;
    let pipelines = get_pipelines(p4info.clone(), pipeline_arg)?;

    // Actions are referenced by id, so make a map.
    let action_by_id: HashMap<u32, &Action> = p4info
        .get_actions()
        .iter()
        .map(|a| (a.get_preamble().id, a))
        .collect();

    let mut output = String::new();
    for (_, tables) in pipelines {
        for table in tables {
            let table_name = &table.get_preamble().name;

            use p4info::MatchField_MatchType::*;

            // Declarations for 'table', as (field_name, type) tuples.
            let mut decls = Vec::new();

            // Basic declaration for each match field.
            for mf in table.get_match_fields() {
                let bt = p4_basic_type(mf.bitwidth, &mf.annotations);

                let full_type = match mf.get_match_type() {
                    EXACT => bt,
                    LPM => format!("({}, bit<32>>)", bt),
                    RANGE | TERNARY => format!("({}, {})", bt, bt),
                    OPTIONAL => format!("Option<{}>", bt),
                    UNSPECIFIED => "()".into(),
                };

                decls.push((mf.name.clone(), full_type));
            }

            // If the match fields are all exact-match, we don't need
            // a priority, otherwise include one.
            if table
                .get_match_fields()
                .iter()
                .any(|mf| mf.get_match_type() != EXACT)
            {
                decls.push(("priority".to_string(), "bit<32>".to_string()));
            }

            // Grab the actions for 'table'.  We only care about
            // actions that we can set through the control plane, so
            // omit DEFAULT_ONLY actions.
            let actions: Vec<_> = table
                .get_action_refs()
                .iter()
                .filter(|ar| ar.scope != p4info::ActionRef_Scope::DEFAULT_ONLY)
                .map(|ar| action_by_id.get(&ar.id).unwrap())
                .collect();

            // If there is just one action and it doesn't have any
            // parameters, then we don't need to include the actions
            // in the relation.
            let needs_actions =
                actions.len() > 1 || (actions.len() == 1 && !actions[0].get_params().is_empty());
            if needs_actions {
                let action_type_name = format!("{}Action", table_name);

                write!(output, "typedef {}", action_type_name)?;
                for (i, a) in actions.iter().enumerate() {
                    write!(
                        output,
                        " {} {}{}",
                        if i == 0 { "=" } else { "|" },
                        action_type_name,
                        a.get_preamble().alias
                    )?;
                    if !a.get_params().is_empty() {
                        let params: String = a
                            .get_params()
                            .iter()
                            .map(|p| {
                                format!("{}: {}", p.name, p4_basic_type(p.bitwidth, &p.annotations))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        write!(output, "{{{}}}", params)?;
                    }
                }
                writeln!(output)?;

                decls.push(("action".to_string(), action_type_name));
            }

            // Ordinarily, we declare the relation to contain structs,
            // but if the relation only has a single member and it's
            // annotated with @nerpa_singleton, declare it as the type
            // of that single member.
            if decls.len() == 1
                && table
                    .get_preamble()
                    .annotations
                    .has_annotation("@nerpa_singleton")
            {
                let (_, full_type) = &decls[0];
                writeln!(output, "output relation {}[{}]", table_name, full_type)?;
            } else {
                writeln!(output, "output relation {}(", table_name)?;
                for (i, (name, full_type)) in decls.iter().enumerate() {
                    let delimiter = if i == decls.len() - 1 { "" } else { "," };
                    writeln!(output, "    {}: {}{}", name, full_type, delimiter)?;
                }
                writeln!(output, ")")?;
            }
        }
    }

    // Create input relations for the digest messages. 
    
    // Map the digest name to its type information.
    use std::collections::HashSet;
    let digest_names: HashSet<&str> = p4info
        .get_digests()
        .iter()
        .map(|d| d.get_preamble().get_name())
        .collect();

    let all_structs = p4info
        .get_type_info()
        .get_structs()
        .clone();

    let mut digest_structs = all_structs.clone();
    digest_structs.retain(|k, _| digest_names.contains(k.as_str()));

    // Define all custom types needed for the input relations.
    let mut typedefs_vec = Vec::new();
    for (k, ds) in digest_structs.iter() {
        let members = ds.get_members();

        for m in members.iter() {
            typedefs_vec.append(&mut extract_p4data_types(&m.get_type_spec().type_spec));
        }
    }

    use std::iter::FromIterator;
    let typedefs_set = HashSet::<String>::from_iter(typedefs_vec);
    for (k, s) in all_structs.iter() {
        if !typedefs_set.contains(k) {
            continue;
        }

        write!(output, "typedef {} = {}{{", k, k)?;
        let members = s.get_members();
        for (i, m) in members.iter().enumerate() {
            let delimiter = if i == members.len() - 1 { "" } else { "," };

            let name = m.get_name();
            let type_spec = &m.get_type_spec().type_spec;
            let full_type = p4data_to_ddlog_type(type_spec);

            write!(output, "{}: {}{}", name, full_type, delimiter);
        }
        writeln!(output, "}}")?;
    }

    // Format the digests as input relations.
    // Write the formatted input relation to the output buffer.
    for (k, ds) in digest_structs.iter() {
        let members = ds.get_members();

        // Store each member as a field for the input relation using (field_name, type).
        let mut fields = Vec::new();
        for m in members.iter() {
            let name = m.get_name();
            let type_spec = &m.get_type_spec().type_spec;
            let full_type = p4data_to_ddlog_type(type_spec);

            fields.push((name, full_type));
        }

        // Write the input relation to the output file.
        writeln!(output, "input relation {}(", k)?;
        for (i, (name, full_type)) in fields.iter().enumerate() {
            let delimiter = if i == fields.len() - 1 { "" } else { "," };
            writeln!(output, "    {}: {}{}", name, full_type, delimiter)?;
        }
        writeln!(output, ")")?;
    }

    let output_filename_os = OsStr::new(output_arg.unwrap());
    let output_filename = output_filename_os.to_string_lossy();
    File::create(output_filename_os)
        .with_context(|| format!("{}: create failed", output_filename))?
        .write_all(output.as_bytes())
        .with_context(|| format!("{}: write failed", output_filename))?;
    
    // If the Rust file was not passed, early return.
    // TODO: Alter the first output argument.
    if rs_output_arg.is_none() {
        return Ok(());
    }

    let mut rs_output = String::new();

    // Map digest to type.
    writeln!(rs_output, "pub fn map_digest_to_type() {{");
    writeln!(rs_output, "   println!(\"Hello world!\");");
    writeln!(rs_output, "}}");

    // Write the `.rs` output file.
    let rs_output_filename_os = OsStr::new(rs_output_arg.unwrap());
    let rs_output_filename = rs_output_filename_os.to_string_lossy();
    File::create(rs_output_filename_os)
        .with_context(|| format!("{}: create failed", rs_output_filename))?
        .write_all(rs_output.as_bytes())
        .with_context(|| format!("{}: write failed", rs_output_filename))?;
    
    // Add a `.toml` file with the dependencies.
    
    Ok(())
}

