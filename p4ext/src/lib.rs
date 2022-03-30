/*!
Wrapper library for the P4 Runtime API.

Provides convenience functions for
[P4 Runtime](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html).
This facilitates communication with the P4 switch.

This crate assumes that Rust bindings, generated from the 
Protobuf files, exist within a `proto` crate. These
can be generated using `build-nerpa.sh`. They provide
a P4 Runtime client, the P4 Runtime data structures, and
P4 Info data structures.
*/
#![warn(missing_docs)]
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

use futures::{SinkExt, StreamExt};

use grpcio::{ChannelBuilder, EnvBuilder, WriteFlags};

use itertools::Itertools;

use proto::p4info;

use proto::p4runtime::{
    ForwardingPipelineConfig,
    ForwardingPipelineConfig_Cookie,
    GetForwardingPipelineConfigRequest,
    MasterArbitrationUpdate,
    MulticastGroupEntry,
    PacketReplicationEngineEntry,
    ReadRequest,
    SetForwardingPipelineConfigRequest,
    SetForwardingPipelineConfigRequest_Action,
    StreamMessageRequest,
    StreamMessageResponse,
    WriteRequest
};

use proto::p4runtime_grpc::P4RuntimeClient;

use proto::p4types;

use protobuf::{Message, RepeatedField};

use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::fs;
use std::process::Command;
use std::string::String;
use std::sync::Arc;

/// An annotation's [location](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-sourcelocation-message>) within a `.p4` file.
#[derive(Clone, Debug, Default)]
pub struct SourceLocation {
    file: String,
    line: i32,
    column: i32,
}

impl From<&p4types::SourceLocation> for SourceLocation {
    fn from(s: &p4types::SourceLocation) -> Self {
        SourceLocation {
            file: s.file.clone(),
            line: s.line,
            column: s.column,
        }
    }
}

impl Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if self.line != 0 {
            write!(f, ":{}", self.line)?;
            if self.column != 0 {
                write!(f, ":{}", self.column)?;
            }
        }
        Ok(())
    }
}

/// Values in an [expression](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-structured-annotations) in a structured annotation.
#[derive(Clone, Debug)]
pub enum ExpressionValue {
    /// String value.
    String(String),
    /// Integer value.
    Integer(i64),
    /// Boolean value.
    Bool(bool),
}

impl From<&p4types::Expression> for ExpressionValue {
    fn from(e: &p4types::Expression) -> Self {
        use p4types::Expression_oneof_value::*;
        match e.value {
            Some(string_value(ref s)) => ExpressionValue::String(s.clone()),
            Some(int64_value(i)) => ExpressionValue::Integer(i),
            Some(bool_value(b)) => ExpressionValue::Bool(b),
            None => todo!(),
        }
    }
}

impl Display for ExpressionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpressionValue::String(s) => write!(f, "\"{}\"", s.escape_debug()),
            ExpressionValue::Integer(i) => write!(f, "{}", i),
            ExpressionValue::Bool(b) => write!(f, "{}", b),
        }
    }
}

/// Maps a name to a value. Possible data type in a structured annotation.
#[derive(Clone, Debug)]
pub struct KeyValuePair(String, ExpressionValue);

impl From<&p4types::KeyValuePair> for KeyValuePair {
    fn from(kvp: &p4types::KeyValuePair) -> Self {
        KeyValuePair(kvp.get_key().into(), kvp.get_value().into())
    }
}

impl Display for KeyValuePair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.0.escape_debug(), self.1)
    }
}

/// Possible [annotation values](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-structured-annotations) for a P4 Runtime entity.
#[derive(Clone, Debug)]
pub enum AnnotationValue {
    /// Empty content can be in an unstructured annotation.
    Empty,
    /// An unstructured annotation can be free-form.
    Unstructured(String),
    /// One of two forms for an expression list.
    Expressions(Vec<ExpressionValue>),
    /// One of two forms for an expression list.
    KeyValuePairs(Vec<KeyValuePair>),
}

impl From<&p4types::ExpressionList> for AnnotationValue {
    fn from(el: &p4types::ExpressionList) -> Self {
        AnnotationValue::Expressions(el.get_expressions().iter().map(|e| e.into()).collect())
    }
}

impl From<&p4types::KeyValuePairList> for AnnotationValue {
    fn from(kvpl: &p4types::KeyValuePairList) -> Self {
        AnnotationValue::KeyValuePairs(kvpl.get_kv_pairs().iter().map(|kvp| kvp.into()).collect())
    }
}

impl From<&p4types::StructuredAnnotation> for AnnotationValue {
    fn from(sa: &p4types::StructuredAnnotation) -> AnnotationValue {
        if sa.has_expression_list() {
            sa.get_expression_list().into()
        } else {
            sa.get_kv_pair_list().into()
        }
    }
}

/// Annotations in a [preamble](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-preamble-message). Maps name to value and optional location.
#[derive(Clone, Debug, Default)]
pub struct Annotations(HashMap<String, (Option<SourceLocation>, AnnotationValue)>);

fn parse_annotations<'a, T, U, V>(
    annotations: T,
    annotation_locs: U,
    structured_annotations: V,
) -> Annotations
where
    T: IntoIterator<Item = &'a String>,
    U: IntoIterator<Item = &'a p4types::SourceLocation>,
    V: IntoIterator<Item = &'a p4types::StructuredAnnotation>,
{
    use AnnotationValue::*;

    // The annotation locations are optional.  Extend them so that we
    // always have one to match up with the annotations.
    let extended_annotation_locs = annotation_locs
        .into_iter()
        .map(|a| Some(a.into()))
        .chain(std::iter::repeat(None));
    let unstructured_annotations =
        annotations
            .into_iter()
            .zip(extended_annotation_locs)
            .map(|(s, source_location)| {
                let s = s.trim_start_matches("@");
                if s.contains("(") && s.ends_with(")") {
                    let index = s.find("(").unwrap();
                    let name = String::from(&s[0..index]);
                    let value = s[index + 1..].strip_suffix(')').unwrap().into();
                    (name, (source_location, Unstructured(value)))
                } else {
                    (s.into(), (source_location, Empty))
                }
            });
    let structured_annotations = structured_annotations.into_iter().map(|x| {
        (
            x.name.clone(),
            (
                if x.has_source_location() {
                    Some(x.get_source_location().into())
                } else {
                    None
                },
                x.into(),
            ),
        )
    });
    Annotations(
        unstructured_annotations
            .chain(structured_annotations)
            .collect(),
    )
}

fn format_structured_annotation<T, U>(f: &mut fmt::Formatter<'_>, values: T) -> fmt::Result
where
    T: Iterator<Item = U>,
    U: Display,
{
    write!(f, "[")?;
    for (i, e) in values.enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{}", e)?;
    }
    write!(f, "]")
}

impl Display for Annotations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Sort annotations by name to ensure predictable output.
        let sorted_annotations = self.0.iter().sorted_by(|a, b| a.0.cmp(b.0));
        for (i, (k, (_, v))) in sorted_annotations.into_iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "@{}", k)?;

            use AnnotationValue::*;
            match v {
                Empty => (),
                Unstructured(s) => write!(f, "({})", s.escape_debug())?,
                Expressions(expressions) => format_structured_annotation(f, expressions.iter())?,
                KeyValuePairs(kvp) => format_structured_annotation(f, kvp.iter())?,
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct Documentation {
    brief: String,
    description: String,
}

impl From<&p4info::Documentation> for Documentation {
    fn from(t: &p4info::Documentation) -> Self {
        Self {
            brief: t.brief.clone(),
            description: t.description.clone(),
        }
    }
}

/// [Preamble](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-preamble-message) describes a P4 entity.
#[derive(Clone, Debug, Default)]
pub struct Preamble {
    /// Unique instance ID for a P4 entity.
    pub id: u32,
    /// Full name of the P4 object, e.g. `c1.c2.ipv4_lpm`.
    pub name: String,
    alias: String,
    annotations: Annotations,
    doc: Documentation,
}

impl From<&p4info::Preamble> for Preamble {
    fn from(p: &p4info::Preamble) -> Self {
        Preamble {
            id: p.id,
            name: p.name.clone(),
            alias: p.alias.clone(),
            annotations: parse_annotations(
                p.get_annotations(),
                p.get_annotation_locations(),
                p.get_structured_annotations(),
            ),
            doc: p.get_doc().into(),
        }
    }
}

/// An enumeration of possible PSA match kinds. Described [here](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-match-format).
#[derive(Clone, PartialEq, Eq)]
pub enum MatchType {
    /// Unspecified.
    Unspecified,
    /// Exact.
    Exact,
    /// Longest prefix.
    Lpm,
    /// Ternary.
    Ternary,
    /// Represents min..max intervals.
    Range,
    /// Optional match field.
    Optional,
    /// Encodes other, architecture-specific match type.
    Other(String),
}

use std::fmt::Debug;

impl Debug for MatchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Display for MatchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MatchType::*;
        let s = match self {
            Unspecified => "unspecified",
            Exact => "exact",
            Lpm => "LPM",
            Ternary => "ternary",
            Range => "range",
            Optional => "optional",
            Other(s) => &s,
        };
        write!(f, "{}", s)
    }
}

/// Data used to construct a lookup key matched in a table.
/// Based on the definition [here](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
/// Public fields are used by external crates to convert data to/from the `MatchField` format.
#[derive(Clone, Debug)]
pub struct MatchField {
    /// Field ID and name.
    // The protobuf representation of MatchField doesn't include a
    // Preamble, but it includes everything in the preamble except
    // 'alias'.  It seems more uniform to just use Preamble here.
    pub preamble: Preamble,
    /// Size in bits of the match field.
    pub bit_width: i32,
    /// Describes the match behavior of the field.
    pub match_type: MatchType,
    type_name: Option<String>,
    // unknown_fields: 
}

impl From<&p4info::MatchField> for MatchField {
    fn from(mf: &p4info::MatchField) -> Self {
        use p4info::MatchField_MatchType::*;
        MatchField {
            preamble: Preamble {
                id: mf.id,
                name: mf.name.clone(),
                alias: mf.name.clone(),
                annotations: parse_annotations(
                    mf.get_annotations(),
                    mf.get_annotation_locations(),
                    mf.get_structured_annotations(),
                ),
                doc: mf.get_doc().into(),
            },
            bit_width: mf.bitwidth,
            match_type: match mf.get_match_type() {
                EXACT => MatchType::Exact,
                LPM => MatchType::Lpm,
                TERNARY => MatchType::Ternary,
                RANGE => MatchType::Range,
                OPTIONAL => MatchType::Optional,
                UNSPECIFIED => {
                    if mf.has_other_match_type() {
                        MatchType::Other(mf.get_other_match_type().into())
                    } else {
                        MatchType::Unspecified
                    }
                }
            },
            type_name: None, // XXX
        }
    }
}

impl Display for MatchField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field {}: bit<{}>", self.preamble.name, self.bit_width)?;
        if let Some(ref type_name) = self.type_name {
            write!(f, " ({})", type_name.escape_debug())?;
        }
        write!(f, " {}-match", self.match_type)?;
        if !self.preamble.annotations.0.is_empty() {
            write!(f, " {}", self.preamble.annotations)?;
        };
        Ok(())
    }
}


fn parse_type_name(pnto: Option<&p4types::P4NamedType>) -> Option<String> {
    pnto.map(|pnt| pnt.name.clone())
}

/// [Runtime parameter](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action) provided by the control plane for an action.
#[derive(Clone, Debug, Default)]
pub struct ActionParam {
    /// Param ID and name.
    // The protobuf representation of Param doesn't include a
    // Preamble but it includes everything in the preamble except
    // 'alias'.  It seems more uniform to just use Preamble here.
    pub preamble: Preamble,
    bit_width: i32,
    type_name: Option<String>,
}

impl From<&p4info::Action_Param> for ActionParam {
    fn from(ap: &p4info::Action_Param) -> Self {
        ActionParam {
            preamble: Preamble {
                id: ap.id,
                name: ap.name.clone(),
                alias: ap.name.clone(),
                annotations: parse_annotations(
                    ap.get_annotations(),
                    ap.get_annotation_locations(),
                    ap.get_structured_annotations(),
                ),
                doc: ap.get_doc().into(),
            },
            bit_width: ap.bitwidth,
            type_name: parse_type_name(ap.type_name.as_ref()),
        }
    }
}

impl Display for ActionParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: bit<{}>", self.preamble.name, self.bit_width)
    }
}

/// [Action](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action) in a match-action table.
#[derive(Clone, Debug, Default)]
pub struct Action {
    /// Action ID, name, and alias.
    pub preamble: Preamble,
    /// Runtime parameters when modifying/inserting a table entry
    /// with this action, provided by the control plane.
    pub params: Vec<ActionParam>,
}

impl From<&p4info::Action> for Action {
    fn from(a: &p4info::Action) -> Self {
        Action {
            preamble: a.get_preamble().into(),
            params: a.get_params().iter().map(|x| x.into()).collect(),
        }
    }
}

impl Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "action {}(", self.preamble.name)?;
        for (p_index, p) in self.params.iter().enumerate() {
            if p_index > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", p)?;
        }
        write!(f, ")")
    }
}

/// Represents a possible action in a table with additional information.
/// Described within [this](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
#[derive(Clone, Debug, Default)]
pub struct ActionRef {
    /// The action.
    pub action: Action,
    /// Means that the action is not TABLE_ONLY.
    pub may_be_default: bool,
    /// Means that the action is not DEFAULT_ONLY.
    pub may_be_entry: bool,
    /// Annotations for the action.
    pub annotations: Annotations,
}

impl ActionRef {
    fn new_from_proto(ar: &p4info::ActionRef, actions: &HashMap<u32, Action>) -> Self {
        ActionRef {
            action: actions.get(&ar.id).unwrap().clone(),
            may_be_default: ar.scope != p4info::ActionRef_Scope::TABLE_ONLY,
            may_be_entry: ar.scope != p4info::ActionRef_Scope::DEFAULT_ONLY,
            annotations: parse_annotations(
                ar.get_annotations(),
                ar.get_annotation_locations(),
                ar.get_structured_annotations(),
            ),
        }
    }
}

impl Display for ActionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.may_be_entry {
            write!(f, "default-only ")?;
        } else if !self.may_be_default {
            write!(f, "not-default ")?;
        }
        write!(f, "{}", self.action)?;
        if !self.annotations.0.is_empty() {
            write!(f, " {}", self.annotations)?;
        };
        Ok(())
    }
}

/// Match-action [table](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
#[derive(Clone, Debug, Default)]
pub struct Table {
    /// Table ID, name, and alias.
    pub preamble: Preamble,
    /// Data to construct lookup key matched.
    pub match_fields: Vec<MatchField>,
    /// Set of possible actions for the table.
    pub actions: Vec<ActionRef>,
    const_default_action: Option<Action>,
    //action_profile: Option<ActionProfile>,
    //direct_counter: Option<DirectCounter>,
    //direct_meter: Option<DirectMeter>,
    max_entries: Option<u64>,
    idle_notify: bool,
    is_const_table: bool,
}

impl Table {
    fn new_from_proto(t: &p4info::Table, actions: &HashMap<u32, Action>) -> Self {
        Table {
            preamble: t.get_preamble().into(),
            match_fields: t.get_match_fields().iter().map(|x| x.into()).collect(),
            actions: t
                .get_action_refs()
                .iter()
                .map(|x| ActionRef::new_from_proto(x, actions))
                .collect(),
            const_default_action: None, // XXX
            max_entries: if t.size > 0 {
                Some(t.size as u64)
            } else {
                None
            },
            idle_notify: t.idle_timeout_behavior
                == p4info::Table_IdleTimeoutBehavior::NOTIFY_CONTROL,
            is_const_table: t.is_const_table,
        }
    }
}

impl Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "table {}:", self.preamble.name)?;
        for mf in &self.match_fields {
            write!(f, "\t{}", mf)?;
        }
        for ar in &self.actions {
            write!(f, "\t{}", ar)?;
        }
        if let Some(max_entries) = self.max_entries {
            write!(f, "\tsize: {}", max_entries)?;
        }
        if let Some(a) = &self.const_default_action {
            write!(f, "\tconst default action {}", a)?;
        }
        if self.is_const_table {
            write!(f, "\tconst table")?;
        }
        if self.idle_notify {
            write!(f, "\tidle notify")?;
        }
        Ok(())
    }
}

/// Represents a P4-programmable switch.
pub struct Switch {
    /// Tables within a switch.
    pub tables: Vec<Table>,
}

impl From<&p4info::P4Info> for Switch {
    fn from(p4i: &p4info::P4Info) -> Self {
        let actions: HashMap<u32, Action> = p4i
            .get_actions()
            .iter()
            .map(|x| (x.get_preamble().id, x.into()))
            .collect();
        let tables: Vec<Table> = p4i
            .get_tables()
            .iter()
            .map(|x| Table::new_from_proto(x, &actions))
            .collect();
        Switch { tables }
    }
}

/// An error received from the dataplane.
#[derive(Debug)]
pub struct P4Error {
    /// Error message.
    pub message: String
}

impl fmt::Display for P4Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Necessary data to test library function.
pub struct TestSetup {
    /// Filepath for p4info binary file.
    pub p4info: String,
    /// Filepath for compiled P4 program as JSON.
    pub json: String,
    /// Opaque data to identify a pipeline config.
    pub cookie: String,
    /// Requested configuration action.
    pub action: String,
    /// P4 device ID.
    pub device_id: u64,
    /// Requested role for controller.
    pub role_id: u64,
    /// Target host and port for the switch.
    pub target: String,
    /// P4 runtime client to send requests.
    pub client: P4RuntimeClient,
    /// Name of the table.
    pub table_name: String,
    /// Name of the action.
    pub action_name: String,
    /// Action parameters mapped to values.
    // TODO: Change this data type.
    pub params_values: HashMap<String, u16>,
    /// Match fields mapped to values.
    // TODO: Change this data type.
    pub match_fields_map: HashMap<String, u16>,
}

impl TestSetup {
    /// Set up a switch for testing.
    pub fn new() -> Self {
        let deps_var = "NERPA_DEPS";
        let switch_path = "behavioral-model/targets/simple_switch_grpc/simple_switch_grpc";

        let nerpa_deps = match env::var(deps_var) {
            Ok(val) => val,
            Err(err) => panic!("Set env var ${} before running tests (error: {})!", deps_var, err),
        };

        let filepath = format!("{}/{}", nerpa_deps, switch_path);
        let mut command = Command::new(filepath);
        command.args(&[
            "--no-p4",
            "--",
            "--grpc-server-addr",
            "0.0.0.0:50051",
            "--cpu-port",
            "1010"
        ]);

        match command.spawn() {
            Ok(child) => println!("server process id: {}", child.id()),
            Err(e) => panic!("server didn't start: {}", e),
        }

        let target = "localhost:50051";
        let env = Arc::new(EnvBuilder::new().build());
        let ch = ChannelBuilder::new(env).connect(target);
        let client = P4RuntimeClient::new(ch);

        let mut params_values : HashMap<String, u16> = HashMap::new();
        params_values.insert("port".to_string(), 11);
        let mut match_fields_map : HashMap<String, u16> = HashMap::new();
        match_fields_map.insert("standard_metadata.ingress_port".to_string(), 11);
        match_fields_map.insert("hdr.vlan.vid".to_string(), 1);


        Self {
            p4info: "examples/vlan/vlan.p4info.bin".to_string(),
            json: "examples/vlan/vlan.json".to_string(),
            cookie: "".to_string(),
            action: "verify-and-commit".to_string(),
            device_id: 0,
            role_id: 0,
            target: target.to_string(),
            client: client,
            table_name: "MyIngress.vlan_incoming_exact".to_string(),
            action_name: "MyIngress.vlan_incoming_forward".to_string(),
            params_values: params_values,
            match_fields_map: match_fields_map,
        }
    }
}

/// Set configuration for the forwarding pipeline.
///
/// This calls the [`SetForwardingPipelineConfig` RPC](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-setforwardingpipelineconfig-rpc).
///
/// # Arguments
/// * `p4info_str` - filepath for the p4info binary file.
/// * `json_str` - filepath for the compiled P4 program's JSON representation.
/// * `cookie_str` - cookie for the forwarding config.
/// * `action_str` - action for the forwarding pipeline.
/// * `device_id` - ID of the P4-enabled device.
/// * `role_id` - the controller's desired role.
/// * `target` - entity hosting P4 Runtime.
/// * `client` - P4 Runtime client.
pub fn set_pipeline_config(
    p4info_str: &str,
    json_str: &str,
    cookie_str: &str,
    action_str: &str,
    device_id: u64,
    role_id: u64,
    target: &str,
    client: &P4RuntimeClient,
) {
    let p4info_os: &OsStr = OsStr::new(p4info_str);
    let mut p4info_file = fs::File::open(p4info_os)
        .unwrap_or_else(|err| panic!("{}: could not open P4Info ({})", p4info_str, err));
    let p4info = Message::parse_from_reader(&mut p4info_file)
        .unwrap_or_else(|err| panic!("{}: could not read P4Info ({})", p4info_str, err));

    let json_filename = OsStr::new(json_str);
    let json = fs::read(json_filename).unwrap_or_else(|err| {
        panic!(
            "{}: could not read json data ({})",
            json_filename.to_string_lossy(),
            err
        )
    });

    let mut config = ForwardingPipelineConfig::new();
    config.set_p4_device_config(json);
    config.set_p4info(p4info);

    if cookie_str != "" {
        let mut cookie_jar = ForwardingPipelineConfig_Cookie::new();
        cookie_jar.set_cookie(str::parse::<u64>(&cookie_str).unwrap());
        config.set_cookie(cookie_jar);
    }

    use SetForwardingPipelineConfigRequest_Action::*;
    let action = match action_str {
        "verify" => VERIFY,
        "verify-and-save" => VERIFY_AND_SAVE,
        "verify-and-commit" => VERIFY_AND_COMMIT,
        _ => RECONCILE_AND_COMMIT,
    };

    let mut set_pipeline_request = SetForwardingPipelineConfigRequest::new();
    set_pipeline_request.set_action(action);
    set_pipeline_request.set_device_id(device_id);
    set_pipeline_request.set_role_id(role_id);
    set_pipeline_request.set_config(config);
    client
        .set_forwarding_pipeline_config(&set_pipeline_request)
        .unwrap_or_else(|err| panic!("{}: failed to set forwarding pipeline ({})", target, err));
}

/// Retrieve configuration for the forwarding pipeline.
///
/// Calls the [`GetForwardingPipelineConfig` RPC](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-getforwardingpipelineconfig-rpc).
///
/// Panics if unable to get configuration from the provided device.
///
/// # Arguments
/// * `device_id` - ID of the P4 device to get config for.
/// * `target` - hardware/software entity hosting P4 Runtime.
/// * `client` - P4 Runtime client.
pub fn get_pipeline_config(
    device_id: u64,
    target: &str,
    client: &P4RuntimeClient
) -> ForwardingPipelineConfig {
    let mut get_pipeline_request = GetForwardingPipelineConfigRequest::new();
    get_pipeline_request.set_device_id(device_id);
    get_pipeline_request.set_response_type(
        proto::p4runtime::GetForwardingPipelineConfigRequest_ResponseType::P4INFO_AND_COOKIE,
    );

    let pipeline_response = client
        .get_forwarding_pipeline_config(&get_pipeline_request)
        .unwrap_or_else(|err| {
            panic!(
                "{}: failed to retrieve forwarding pipeline ({})",
                target, err
            )
        });
    let pipeline = pipeline_response.get_config();
    if !pipeline.has_p4info() {
        panic!("{}: device did not return P4Info", target);
    }
    pipeline.clone()
}

/// Build an update for a [table entry](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table-entry).
/// 
/// # Arguments
/// * `update_type` - the type of update: insert, modify, or delete.
/// * `table_id` - the ID of the table to update.
/// * `table_action` - the action to execute on match.
/// * `field_matches` - values to match on.
/// * `priority` - used to order entries.
pub fn build_table_entry_update(
    update_type: proto::p4runtime::Update_Type,
    table_id: u32,
    table_action: proto::p4runtime::TableAction,
    field_matches: Vec<proto::p4runtime::FieldMatch>,
    priority: i32, 
) -> proto::p4runtime::Update {
    let mut table_entry = proto::p4runtime::TableEntry::new();
    table_entry.set_table_id(table_id);
    table_entry.set_action(table_action);
    table_entry.set_field_match(protobuf::RepeatedField::from_vec(field_matches));
    table_entry.set_priority(priority);

    let mut entity = proto::p4runtime::Entity::new();
    entity.set_table_entry(table_entry);

    let mut update = proto::p4runtime::Update::new();
    update.set_field_type(update_type);
    update.set_entity(entity);

    update
}

/// Write a set of table updates to the switch.
///
/// Calls the [`Write` RPC](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-write-rpc>).
///
/// # Arguments
/// * `updates` - updates to be written.
/// * `device_id` - ID for the P4 device to write to.
/// * `role_id` - role of the controller.
/// * `target` - entity hosting P4 runtime, used for debugging.
/// * `client` - P4 Runtime client.
pub fn write(
    updates: Vec<proto::p4runtime::Update>,
    device_id: u64,
    role_id: u64,
    target: &str,
    client: &P4RuntimeClient,
) -> Result<(), P4Error> {
    let mut write_request = WriteRequest::new();
    write_request.set_device_id(device_id);
    write_request.set_role_id(role_id);
    write_request.set_updates(RepeatedField::from_vec(updates));

    match client.write(&write_request) {
        Ok(_w) => Ok(()),
        Err(e) => Err(P4Error{message: format!("{}, {}, {}: failed to write request ({})", target, device_id, role_id, e)}), 
    }
}

/// Retrieve one or more P4 entities.
///
/// Calls the [`Read RPC`](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-read-rpc).
///
/// # Arguments
/// * `entities` - a list of P4 entities, each acting as a query filter.
/// * `device_id` - uniquely identifies the target P4 device.
/// * `client` - P4 Runtime client.
pub async fn read(
    entities: Vec<proto::p4runtime::Entity>,
    device_id: u64,
    client: &P4RuntimeClient,
) -> Result<Vec<proto::p4runtime::Entity>, P4Error> {
    let mut read_request = ReadRequest::new();
    read_request.set_device_id(device_id);
    read_request.set_entities(RepeatedField::from_vec(entities));

    let mut stream = match client.read(&read_request) {
        Ok(r) => r.enumerate(),
        Err(e) => return Err(P4Error {message: format!("{}: failed to read request({})", device_id, e)}),
    };

    let (_, response) = stream.next().await.unwrap();
    match response {
        Ok(r) => Ok(r.get_entities().to_vec()),
        Err(e) => Err(P4Error{ message: format!("{}: received invalid response({})", device_id, e)}),
    }
}

/// Return the response for a request over the streaming channel.
///
/// Calls the `StreamChannel` RPC. This API call is
/// used for session management and packet I/O,
/// among other [stream messages](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-p4runtime-stream-messages).
///
/// # Arguments
/// * `request` - request to send over the channel.
/// * `client` - P4 Runtime client.
pub async fn stream_channel_request(
    request: StreamMessageRequest,
    client: &P4RuntimeClient,
) -> Result<StreamMessageResponse, grpcio::Error> {
    let (mut sink, mut receiver) = client.stream_channel().unwrap();

    let send_result = sink.send((request, WriteFlags::default())).await;
    match send_result {
        Ok(_) => {},
        Err(e) => return Err(e),
    };

    receiver.next().await.unwrap()
}

/// Send a master arbitration update to the switch.
///
/// Set the controller as master. Described [here](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-client-arbitration-and-controller-replication).
///
/// # Arguments
/// * `device_id` - ID for the P4 device.
/// * `client` - P4 runtime client.
pub async fn master_arbitration_update(
    device_id: u64,
    client: &P4RuntimeClient
) -> Result<StreamMessageResponse, grpcio::Error> {
    let mut update = MasterArbitrationUpdate::new();
    update.set_device_id(device_id);

    let mut request = StreamMessageRequest::new();
    request.set_arbitration(update);

    stream_channel_request(request, client).await
}

/// Build an update for a [digest entry](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-digestentry).
///
/// # Arguments
/// * `digest_id` - ID of the P4 device.
/// * `max_timeout_ns` - maximum server delay for a digest message, in nanoseconds.
/// * `max_list_size` - maxmum number of digest messages sent in a single DigestList.
/// * `ack_timeout_ns` - timeout the server waits for digest list acknowledgement before sending more messages, in nanoseconds.
pub fn build_digest_entry_update(
    digest_id: u32,
    max_timeout_ns: i64,
    max_list_size: i32,
    ack_timeout_ns: i64,
) -> proto::p4runtime::Update {
    let mut digest_config = proto::p4runtime::DigestEntry_Config::new();
    digest_config.set_max_timeout_ns(max_timeout_ns);
    digest_config.set_max_list_size(max_list_size);
    digest_config.set_ack_timeout_ns(ack_timeout_ns);

    let mut digest_entry = proto::p4runtime::DigestEntry::new();
    digest_entry.set_digest_id(digest_id);
    digest_entry.set_config(digest_config);

    let mut entity = proto::p4runtime::Entity::new();
    entity.set_digest_entry(digest_entry);

    let mut update = proto::p4runtime::Update::new();
    update.set_entity(entity);
    update.set_field_type(proto::p4runtime::Update_Type::INSERT);

    update
}

/// Return an update that modifies a multicast group.
/// The update can be directly passed to `write`.
///
/// Part of a [multicast group entry](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-multicastgroupentry).
///
/// # Arguments
/// * `update_type` - one of insert, modify, or delete.
/// * `group_id` - ID of the multicast group to change.
/// * `replicas` - replicas, with egress ports, to program for the multicast group.
pub fn build_multicast_write(
    update_type: proto::p4runtime::Update_Type,
    group_id: u32,
    replicas: Vec<proto::p4runtime::Replica>,
) -> proto::p4runtime::Update {
    let mut multicast_entry = MulticastGroupEntry::new();
    multicast_entry.set_multicast_group_id(group_id);
    multicast_entry.set_replicas(RepeatedField::from_vec(replicas));

    let mut pre_entry = PacketReplicationEngineEntry::new();
    pre_entry.set_multicast_group_entry(multicast_entry);

    let mut entity = proto::p4runtime::Entity::new();
    entity.set_packet_replication_engine_entry(pre_entry);

    let mut update = proto::p4runtime::Update::new();
    update.set_field_type(update_type);
    update.set_entity(entity);

    update
}

/// Return an entity that can be used to read a multicast group.
/// The entity can be wrapped in a `Vec` and passed to `read`.
///
/// Part of a [multicast group entry](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-multicastgroupentry).
///
/// # Arguments
/// * `group_id` - ID of the multicast group to read.
pub fn build_multicast_read(
    group_id: u32,
) -> proto::p4runtime::Entity {
    let mut multicast_entry = MulticastGroupEntry::new();
    multicast_entry.set_multicast_group_id(group_id);

    let mut pre_entry = PacketReplicationEngineEntry::new();
    pre_entry.set_multicast_group_entry(multicast_entry);

    let mut entity = proto::p4runtime::Entity::new();
    entity.set_packet_replication_engine_entry(pre_entry);

    entity
}
