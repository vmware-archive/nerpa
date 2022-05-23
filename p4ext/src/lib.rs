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

use grpcio::{ChannelBuilder, EnvBuilder, WriteFlags, RpcStatusCode};

use itertools::Itertools;

use anyhow::{Context, Result};

use proto::p4info;

use proto::p4runtime::{
    FieldMatch_Exact,
    FieldMatch_LPM,
    FieldMatch_Optional,
    FieldMatch_Range,
    FieldMatch_Ternary,
    FieldMatch_oneof_field_match_type,
    ForwardingPipelineConfig,
    ForwardingPipelineConfig_Cookie,
    GetForwardingPipelineConfigRequest,
    MasterArbitrationUpdate,
    PacketReplicationEngineEntry,
    ReadRequest,
    SetForwardingPipelineConfigRequest,
    SetForwardingPipelineConfigRequest_Action,
    StreamMessageRequest,
    StreamMessageResponse,
    TableAction_oneof_type,
    WriteRequest,
};

use proto::p4runtime_grpc::P4RuntimeClient;

use proto::p4types;

use protobuf::{Message, RepeatedField};

use std::collections::{BTreeSet, HashMap};
use std::convert::{TryFrom, TryInto};
use std::env;
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::fs;
use std::process::Command;
use std::string::String;
use std::sync::Arc;

use thiserror::Error;

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

/// Annotations in a [preamble](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-preamble-message). Maps from an annotation name (omitting the `@` prefix) to its optional source location and value.
#[derive(Clone, Debug, Default)]
pub struct Annotations(pub HashMap<String, (Option<SourceLocation>, AnnotationValue)>);

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

/// Documentation for a P4 entity.
#[derive(Clone, Debug, Default)]
pub struct Documentation {
    /// A brief description of the item that this documents
    pub brief: String,

    /// An extended description, possibly with multiple sentences and paragraphs.
    pub description: String,
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
    /// Full name of the P4 entity, e.g. `c1.c2.ipv4_lpm`.
    pub name: String,
    /// Alternate name of the P4 entity.
    pub alias: String,
    /// Annotations for the P4 entity.
    pub annotations: Annotations,
    /// Documentation for the P4 entity.
    pub doc: Documentation,
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
    LPM,
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
            LPM => "LPM",
            Ternary => "ternary",
            Range => "range",
            Optional => "optional",
            Other(s) => &s,
        };
        write!(f, "{}", s)
    }
}

/// Returns the DDlog type to use for a `bitwidth`-bit P4 value annotated with 'annotations',
/// e.g. `"bit<16>"`.
///
/// Call via [`MatchField::p4_basic_type()`] or [`Param::p4_basic_type()`].
fn p4_basic_type(bitwidth: i32, annotations: &Annotations) -> String {
    if is_nerpa_bool(bitwidth, annotations) {
        "bool".into()
    } else {
        format!("bit<{}>", bitwidth)
    }
}

/// Returns true if a `bitwidth`-bit field with the given `annotations` should be represented in
/// DDlog as `bool`.  (Otherwise such a field should be represented as `bit<bitwidth>`.)
fn is_nerpa_bool(bitwidth: i32, annotations: &Annotations) -> bool {
    bitwidth == 1 && annotations.0.contains_key("nerpa_bool")
}

/// A field in a [`Table`], including its width in bits and what kind of matching against it is
/// allowed.
/// Based on the definition [here](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
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
    // unknown_fields: 
}

impl MatchField {
    /// Returns the basic DDlog type to use for this `MatchField`, one of `"bit<N>"` or `"bool"`.
    pub fn p4_basic_type(&self) -> String {
        p4_basic_type(self.bit_width, &self.preamble.annotations)
    }

    /// Returns true if this `Matchfield` should be represented in DDlog as a bool, false
    /// otherwise.
    pub fn is_nerpa_bool(&self) -> bool {
        is_nerpa_bool(self.bit_width, &self.preamble.annotations) && self.match_type == MatchType::Exact
    }

    /// Returns the full DDlog type for this `MatchField`.  Whereas [`Self::p4_basic_type()`]
    /// always yields a simple type for a value of the field being matched, this yields a type that
    /// can fully express the kind of matching.  For example, ternary matching a 5-bit field
    /// requires a value and a mask, which we represent in DDlog as the returned tuple type
    /// `"(bit<5>, bit<5>)"`.
    pub fn p4_full_type(&self) -> String {
        let bt = self.p4_basic_type();

        use MatchType::*;
        match self.match_type {
            Exact => bt,
            LPM => format!("({}, bit<32>)", bt),
            Range | Ternary => format!("({}, {})", bt, bt),
            Optional => format!("Option<{}>", bt),
            Unspecified | Other(_) => "()".into(),
        }
    }
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
                LPM => MatchType::LPM,
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
        }
    }
}

impl Display for MatchField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field {}: bit<{}>", self.preamble.name, self.bit_width)?;
        write!(f, " {}-match", self.match_type)?;
        if !self.preamble.annotations.0.is_empty() {
            write!(f, " {}", self.preamble.annotations)?;
        };
        Ok(())
    }
}

/// How a [`FieldMatch`] matches against a [`MatchField`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum FieldMatchType {
    /// Field must contain exactly `.0`.
    Exact(FieldValue),

    /// Field must match the bits in `value` that have 1-bits in `mask`.
    Ternary {
        /// Bits that must match.  0-bits in `mask` must have corresponding 0-bits in `value.
        value: FieldValue,

        /// Each 1-bit indicates that the corresponding bit in `value` must match the value
        /// extracted from a packet.
        mask: FieldValue
    },

    /// The high-order `plen` bits of the field must match the `plen` least-significant bits of
    /// `value`.
    LPM {
        /// Value to match, in the least-significant `plen` bits.
        value: FieldValue,

        /// Number of bits that must match.
        plen: usize
    },

    /// The field must be between `.0` and `.1`, inclusive.
    Range(FieldValue, FieldValue),

    /// The field must contain exactly `.0`.
    Optional(FieldValue)
}
impl Display for FieldMatchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use FieldMatchType::*;
        match self {
            Exact(value) | Optional(value) => write!(f, "{}", value),
            Ternary { value, mask } => write!(f, "{}/{}", value, mask),
            LPM { value, plen } => write!(f, "{}/{}", value, plen),
            Range(low, high) => write!(f, "{}...{}", low, high)
        }
    }
}

/// A predicate for matching against the value of a field extracted from a packet.  A [`TableKey`]
/// matches a packet if all of its `FieldMatch`es evaluate to true.
///
/// Each `FieldMatch` is associated with a [`MatchField`].  The [`match_type`](Self::match_type) in
/// a `FieldMatch` must correspond to the [`match_type`](MatchField::match_type) in its associated
/// [`MatchField`].  For example, if a [`MatchField`] has a `MatchField::match_type` of
/// [`MatchType::Exact`], then any `FieldMatch` associated with it must have a
/// [`FieldMatch::match_type`] of [`FieldMatchType::Exact`].
///
/// For a "don't-care" predicate, that is, one that always yields true regardless of the field's
/// value, `FieldMatch` should not be used at all.  Instead, its containing [`TableKey`] should
/// omit the `FieldMatch` entirely.
///
/// Based on the [P4Runtime
/// specification](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-match-format).
///
/// # To-do
///
/// Possibly, `FieldMatch` should take [read/write
/// symmetry](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-read-write-symmetry)
/// into account for the purpose of equality and hashing, for example by enforcing invariants in
/// constructors.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FieldMatch {
    /// Identifies the corresponding [`MatchField`] by its [`Preamble::id`].
    pub field_id: u32,

    /// Specific matching requirement.
    pub match_type: FieldMatchType
}
impl FieldMatch {
    fn _try_from(fm: &proto::p4runtime::FieldMatch) -> Result<Self> {
        let field_id = fm.field_id;
        match &fm.field_match_type {
            Some(FieldMatch_oneof_field_match_type::exact(FieldMatch_Exact { value, .. }))
                => Ok(FieldMatch { field_id, match_type: FieldMatchType::Exact(value.try_into()?)}),

            Some(FieldMatch_oneof_field_match_type::ternary(FieldMatch_Ternary { value, mask, .. }))
                => {
                    let value: FieldValue = value.try_into()?;
                    let mask: FieldValue = mask.try_into()?;
                    if (value.0 & !mask.0) != 0 {
                        Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                            .context(format!("P4 field value {} has 1-bits not in mask {}", value, mask))
                    } else {
                        Ok(FieldMatch { field_id, match_type: FieldMatchType::Ternary { value, mask }})
                    }
                }

            Some(FieldMatch_oneof_field_match_type::lpm(FieldMatch_LPM { value, prefix_len, .. }))
                => {
                    let value: FieldValue = value.try_into()?;
                    let plen = *prefix_len;
                    if plen < 0 || plen > 128 {
                        Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                            .context(format!("P4 prefix_len {} outside supported range [0,128]", plen))
                    } else if plen < 128 && (value.0 >> plen) != 0 {
                        Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                            .context(format!("P4 field value {} has 1-bits not in prefix_len {}", value, plen))
                    } else {
                        Ok(FieldMatch { field_id, match_type: FieldMatchType::LPM { value, plen: plen as usize }})
                    }
                }

            Some(FieldMatch_oneof_field_match_type::range(FieldMatch_Range { low, high, .. }))
                => {
                    let low: FieldValue = low.try_into()?;
                    let high: FieldValue = high.try_into()?;
                    if high.0 < low.0 {
                        Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                            .context(format!("P4 range match {}...{} has high less than low", low, high))
                    } else {
                        Ok(FieldMatch { field_id, match_type: FieldMatchType::Range(low, high)})
                    }
                }

            Some(FieldMatch_oneof_field_match_type::optional(FieldMatch_Optional { value, .. }))
                => Ok(FieldMatch { field_id, match_type: FieldMatchType::Optional(value.try_into()?)}),

            Some(FieldMatch_oneof_field_match_type::other(_))
                => Err(Error(RpcStatusCode::UNIMPLEMENTED))
                .context(format!("P4 'other' match type is not supported")),

            None => Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                .context(format!("missing P4 FieldMatch"))
        }
    }
}
impl TryFrom<&proto::p4runtime::FieldMatch> for FieldMatch {
    type Error = anyhow::Error;

    fn try_from(fm: &proto::p4runtime::FieldMatch) -> Result<Self> {
        FieldMatch::_try_from(fm).with_context(|| format!("parse error in \"{:?}\"", fm))
    }
}
impl From<&FieldMatch> for proto::p4runtime::FieldMatch {
    fn from(fm: &FieldMatch) -> proto::p4runtime::FieldMatch {
        let (unknown_fields, cached_size) = Default::default();
        proto::p4runtime::FieldMatch {
            field_id: fm.field_id,
            field_match_type: {
                let (unknown_fields, cached_size) = Default::default();
                Some(match fm.match_type {
                    FieldMatchType::Exact(value) => FieldMatch_oneof_field_match_type::exact(
                        FieldMatch_Exact { value: value.into(), unknown_fields, cached_size }),
                    FieldMatchType::Ternary { value, mask } => FieldMatch_oneof_field_match_type::ternary(
                        FieldMatch_Ternary { value: value.into(), mask: mask.into(), unknown_fields, cached_size }),
                    FieldMatchType::LPM { value, plen } => FieldMatch_oneof_field_match_type::lpm(
                        FieldMatch_LPM { value: value.into(), prefix_len: plen as i32, unknown_fields, cached_size }),
                    FieldMatchType::Range(low, high) => FieldMatch_oneof_field_match_type::range(
                        FieldMatch_Range { low: low.into(), high: high.into(), unknown_fields, cached_size }),
                    FieldMatchType::Optional(value) => FieldMatch_oneof_field_match_type::optional(
                        FieldMatch_Optional { value: value.into(), unknown_fields, cached_size })
                })
            },
            unknown_fields, cached_size
        }
    }
}
impl Display for FieldMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.match_type)
    }
}

/// One port in the set of destinations for a multicast group.
///
/// Based on the [P4Runtime
/// specification](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-multicastgroupentry).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Replica {
    /// The output port.
    pub egress_port: u32,

    /// The "instance" for the packet when it is sent to the egress pipeline.  The egress pipeline
    /// can use this value to distinguish otherwise identical copies of the same packet.  (This is
    /// most useful when two `Replica`s have the same `egress_port`.  See the documentation for
    /// `egress_rid` in the [BMv2 simple switch
    /// documentation](https://github.com/p4lang/behavioral-model/blob/main/docs/simple_switch.md).)
    pub instance: u32
}
impl From<&proto::p4runtime::Replica> for Replica {
    fn from(r: &proto::p4runtime::Replica) -> Self {
        Self { egress_port: r.egress_port, instance: r.instance }
    }
}
impl From<&Replica> for proto::p4runtime::Replica {
    fn from(r: &Replica) -> Self {
        proto::p4runtime::Replica { egress_port: r.egress_port, instance: r.instance, ..Default::default() }
    }
}

/// Associates a multicast group ID with a set of replicas.
///
/// Based on the [P4Runtime
/// specification](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-multicastgroupentry).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MulticastGroupEntry {
    /// Group ID.  A value of zero acts as a wildcard for read operations and is not acceptable for
    /// write operations.
    pub multicast_group_id: MulticastGroupId,

    /// Set of replicas.
    pub replicas: BTreeSet<Replica>
}
impl From<&proto::p4runtime::MulticastGroupEntry> for MulticastGroupEntry {
    fn from(mge: &proto::p4runtime::MulticastGroupEntry) -> MulticastGroupEntry {
        MulticastGroupEntry {
            multicast_group_id: mge.multicast_group_id,
            replicas: mge.replicas.iter().map(|r| r.into()).collect()
        }
    }
}
impl From<&MulticastGroupEntry> for proto::p4runtime::MulticastGroupEntry {
    fn from(mge: &MulticastGroupEntry) -> proto::p4runtime::MulticastGroupEntry {
        let (unknown_fields, cached_size) = Default::default();
        proto::p4runtime::MulticastGroupEntry {
            multicast_group_id: mge.multicast_group_id,
            replicas: mge.replicas.iter().map(|r| r.into()).collect(),
            unknown_fields, cached_size
        }
    }
}

/// A value of a packet field.  The field's width in bits is not specified.
///
/// This is currently implement as `u128`, which is big enough for the values we care about
/// currently.  An arbitrary-precision type would be more flexible.
///
/// Equivalent to the [P4Runtime bytestring
/// type](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-bytestrings) except for the
/// width restriction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldValue(pub u128);
impl TryFrom<&Vec<u8>> for FieldValue {
    type Error = anyhow::Error;

    fn try_from(fv: &Vec<u8>) -> Result<Self> {
        if fv.is_empty() {
            Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                .context(format!("0-length P4 field value"))
        } else {
            let mut x = 0;
            for &digit in fv {
                if x >= (1u128 << 120) {
                    return Err(Error(RpcStatusCode::OUT_OF_RANGE))
                        .context(format!("P4 field value exceeds 128-bit maximum supported length"));
                }
                x = (x << 8) | (digit as u128);
            }
            Ok(FieldValue(x))
        }
    }
}

impl From<FieldValue> for Vec<u8> {
    fn from(fv: FieldValue) -> Vec<u8> {
        let mut value = fv.0;
        let mut v: Vec<u8> = Vec::new();
        loop {
            v.push((value & 0xff) as u8);
            value >>= 8;
            if value == 0 {
                v.reverse();
                return v
            }
        }
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            write!(f, "0")
        } else {
            write!(f, "0x{:x}", self.0)
        }
    }
}

/// Identifier for a P4Runtime multicast group.
pub type MulticastGroupId = u32;

/// Identifier for a P4Runtime table.
pub type TableId = u32;

/// The value passed for a parameter to an action, that is, an argument.
///
/// Based on the [P4Runtime `Param`
/// type](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action-specification),
/// which is not to be confused with the [P4Info `Param`
/// type](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action), which specifies
/// what values are acceptable.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ActionParam {
    /// Identifies the [`Param`] by its [`Preamble::id`].
    pub param_id: u32,

    /// Argument value supplied for the [`Param`].
    pub value: FieldValue
}
impl TryFrom<&proto::p4runtime::Action_Param> for ActionParam {
    type Error = anyhow::Error;

    fn try_from(ap: &proto::p4runtime::Action_Param) -> Result<Self> {
        Ok(ActionParam {
            param_id: ap.param_id,
            value: (&ap.value).try_into()?
        })
    }
}
impl From<&ActionParam> for proto::p4runtime::Action_Param {
    fn from(ap: &ActionParam) -> proto::p4runtime::Action_Param {
        let (unknown_fields, cached_size) = Default::default();
        proto::p4runtime::Action_Param {
            param_id: ap.param_id,
            value: ap.value.into(),
            unknown_fields, cached_size
        }
    }
}

/// The action to invoke in a [`TableEntry`].
///
/// Based on [the `params` in P4Runtime
/// `Action`](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action-specification).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TableAction {
    /// Identifies the [`Action`] by its [`Preamble::id`].
    pub action_id: u32,

    /// Arguments to the action.
    pub params: Vec<ActionParam>
}
impl TryFrom<&proto::p4runtime::TableAction> for TableAction {
    type Error = anyhow::Error;

    fn try_from(ta: &proto::p4runtime::TableAction) -> Result<Self> {
        match &ta.field_type {
            Some(TableAction_oneof_type::action(a)) =>
                Ok(TableAction {
                    action_id: a.action_id,
                    params: a.params.iter().map(|x| x.try_into()).collect::<Result<Vec<_>>>()?,
                }),
            Some(_) => Err(Error(RpcStatusCode::UNIMPLEMENTED))
                .context(format!("unsupported TableAction type {:?}", ta)),
            None => Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                .context(format!("missing TableAction type"))
        }
    }
}
impl From<&TableAction> for proto::p4runtime::TableAction {
    fn from(ta: &TableAction) -> proto::p4runtime::TableAction {
        let (unknown_fields, cached_size) = Default::default();
        proto::p4runtime::TableAction {
            field_type: Some({
                let (unknown_fields, cached_size) = Default::default();
                TableAction_oneof_type::action(proto::p4runtime::Action {
                    action_id: ta.action_id,
                    params: ta.params.iter().map(|param| param.into()).collect(),
                    unknown_fields, cached_size
                })
            }),
            unknown_fields, cached_size
        }
    }
}

/// A [`grpcio::RpcStatusCode`] wrapper that implements [`std::error::Error`], to allow it
/// to be used with [`anyhow::error`].
#[derive(Error, Debug)]
#[error("{}", .0)]
pub struct Error(pub RpcStatusCode);

/// Key data within a [`TableEntry`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TableKey {
    /// Identifies the [`Table`] by its [`Preamble::id`].
    pub table_id: TableId,

    /// The fields and values against which this table entry matches.  Each [`FieldMatch`] element
    /// of this vector must correspond to a [`MatchField`] in this key's [`Table`].  If one or more
    /// fields are to be wildcarded, that is, they are "don't-cares", then those fields must be
    /// omitted from `matches`.
    pub matches: Vec<FieldMatch>,

    /// Matching priority.  Higher numerical values indicate higher priorities.
    ///
    /// The priority member is always present, but some tables don't use it (see
    /// [`TableEntry::has_priority`]).
    pub priority: i32,

    /// True, if this `TableKey` represents the table's default action.  If true, then `matches`
    /// must be empty and `priority` must be 0.
    pub is_default_action: bool,
}
#[derive(Clone, Debug, PartialEq)]

/// Value data within a [`TableEntry`].
pub struct TableValue {
    /// The action to be taken when this entry is matched.  `None` is not allowed within a real
    /// [`TableEntry`], only for specifying an entry to be deleted.
    pub action: Option<TableAction>,

    /// Arbitrary controller-specified metadata.  Deprecated by P4Runtime in favor of `metadata`.
    pub controller_metadata: u64,

    /// Arbitrary controller-specified metadata.
    pub metadata: Vec<u8>
}

/// An entry within a [`Table`].
#[derive(Clone, Debug, PartialEq)]
pub struct TableEntry {
    /// Key.
    pub key: TableKey,

    /// Value.
    pub value: TableValue
}
impl TableEntry {
    fn _try_from(te: &proto::p4runtime::TableEntry) -> Result<Self> {
        if te.is_default_action {
            if !te.field_match.is_empty() {
                return Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                    .context(format!("'is_default_action' is true but 'match' is {:?}", te.field_match));
            }
            if te.priority != 0 {
                return Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                    .context(format!("'is_default_action' is true but 'priority' is {}", te.priority));
            }
            if te.idle_timeout_ns != 0 {
                return Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                    .context(format!("'is_default_action' is true but 'idle_timeout_ns' is {}", te.idle_timeout_ns));
            }
            if te.time_since_last_hit.is_some() {
                return Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                    .context(format!("'is_default_action' is true but 'time_since_last_hit' is {:?}", te.time_since_last_hit));
            }
        }

        Ok(TableEntry {
            key: TableKey {
                table_id: te.table_id,
                matches: te.field_match.iter().map(|fm| fm.try_into()).collect::<Result<Vec<_>>>()?,
                priority: te.priority,
                is_default_action: te.is_default_action
            },
            value: TableValue {
                action: match te.action.clone().into_option() {
                    Some(table_action) => Some((&table_action).try_into()?),
                    None => None
                },
                controller_metadata: te.controller_metadata,
                metadata: te.metadata.clone(),
            }
        })
    }
}
impl TryFrom<&proto::p4runtime::TableEntry> for TableEntry {
    type Error = anyhow::Error;

    fn try_from(te: &proto::p4runtime::TableEntry) -> Result<Self> {
        TableEntry::_try_from(te).with_context(|| format!("parse error in \"{:?}\"", te))
    }
}
impl From<&TableEntry> for proto::p4runtime::TableEntry {
    fn from(te: &TableEntry) -> proto::p4runtime::TableEntry {
        let (meter_config, counter_data, meter_counter_data, idle_timeout_ns, time_since_last_hit, unknown_fields, cached_size)
            = Default::default();
        proto::p4runtime::TableEntry {
            table_id: te.key.table_id,
            field_match: te.key.matches.iter().map(|fm| fm.into()).collect(),
            action: (&te.value.action).as_ref().map(|ta| ta.into()).into(),
            priority: te.key.priority,
            controller_metadata: te.value.controller_metadata,
            meter_config,
            counter_data,
            meter_counter_data,
            is_default_action: te.key.is_default_action,
            idle_timeout_ns,
            time_since_last_hit,
            metadata: te.value.metadata.clone(),
            unknown_fields,
            cached_size
        }
    }
}

#[cfg(feature = "ofp4")]
use differential_datalog::record::{IntoRecord, Name, Record};

#[cfg(feature = "ofp4")]
impl MatchField {
    /// Returns a Record for a DDlog value that matches FieldMatch 'fm' against MatchField 'self'.
    /// If 'fm' is None, then the returned Record represents a don't-care.
    pub fn to_record(&self, fm: Option<&FieldMatch>) -> Result<Record> {
        match fm {
            Some(fm) => match (&self.match_type, &fm.match_type) {
                (MatchType::Exact, FieldMatchType::Exact(value)) => Ok(
                    if self.is_nerpa_bool() {
                        Record::Bool(value.0 != 0)
                    } else {
                        value.0.into_record()
                    }),
                (MatchType::LPM, FieldMatchType::LPM { value, plen })
                    => Ok(Record::Tuple(vec![value.0.into_record(), Record::Int((*plen).into())])),
                (MatchType::Ternary, FieldMatchType::Ternary { value, mask })
                    => Ok(Record::Tuple(vec![value.0.into_record(), mask.0.into_record()])),
                (MatchType::Range, FieldMatchType::Range(low, high))
                    => Ok(Record::Tuple(vec![low.0.into_record(), high.0.into_record()])),
                (MatchType::Optional, FieldMatchType::Optional(value))
                    => Ok(Record::NamedStruct(Name::from("ddlog_std::Some"),
                                              vec![(Name::from("x"), value.0.into_record())])),
                (MatchType::Unspecified, _)
                    => Err(Error(RpcStatusCode::UNIMPLEMENTED))
                    .context(format!("unspecified match not supported")),
                (MatchType::Other(_), _)
                    => Err(Error(RpcStatusCode::UNIMPLEMENTED))
                    .context(format!("other match not supported")),
                _ => Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                    .context(format!("MatchField {} not compatible with FieldMatch {}", self, fm))
            },

            // Don't-care value.
            None => {
                let zero = || 0u32.into_record();
                match self.match_type {
                    MatchType::Exact => Err(Error(RpcStatusCode::INVALID_ARGUMENT))
                        .context(format!("cannot use don't-care for exact-match")),
                    MatchType::LPM => Ok(Record::Tuple(vec![zero(), zero()])),
                    MatchType::Ternary => Ok(Record::Tuple(vec![zero(), zero()])),
                    MatchType::Range => Ok(Record::Tuple(vec![zero(), self.bit_width.into_record()])),
                    MatchType::Optional => Ok(Record::NamedStruct(Name::from("ddlog_std::None"), vec![])),
                    MatchType::Unspecified | MatchType::Other(_) => Ok(Record::Tuple(vec![])),
                }
            }
        }
    }
}

#[cfg(feature = "ofp4")]
impl TableEntry {
    /// Converts this `TableEntry` into a DDlog Record.  The caller must specify the [`Table`] that
    /// the entry is inside.
    pub fn to_record(&self, table: &Table) -> Result<Record> {
        let mut values: Vec<(Name, Record)> = Vec::new();
        for mf in &table.match_fields {
            let fm = self.key.matches.iter().find(|fm| fm.field_id == mf.preamble.id);
            values.push((Name::Owned(mf.preamble.name.clone()), mf.to_record(fm)?));
        }
        if table.has_priority() {
            values.push((Name::from("priority"), self.key.priority.into_record()));
        }
        match &self.value.action {
            Some(TableAction { action_id, params }) => {
                // Find the ActionRef corresponding to 'action_id'.
                let ar = match table.actions.iter().find(|ar| ar.action.preamble.id == *action_id) {
                    Some(ar) => ar,
                    None => return Err(Error(RpcStatusCode::NOT_FOUND)).context(format!("TableEntry {:?} references action not in table", self))
                };

                if ar.action.params.len() == 0 && table.entry_actions().count() == 1 {
                    // This action doesn't have any parameters, and it's the only action.  Don't
                    // include it in the output.
                } else {
                    let action_name = format!("{}Action{}", table.base_name(), ar.action.preamble.alias);
                    let mut param_values: Vec<(Name, Record)> = Vec::new();
                    for p in &ar.action.params {
                        let arg = match params.iter().find(|arg| arg.param_id == p.preamble.id) {
                            Some(arg) => arg,
                            None => return Err(Error(RpcStatusCode::INVALID_ARGUMENT)).context(format!("table entry lacks argument for parameter {:?}", p))?
                        };
                        let record = if p.is_nerpa_bool() {
                            Record::Bool(arg.value.0 != 0)
                        } else {
                            Record::Int(arg.value.0.into())
                        };
                        param_values.push((Name::Owned(p.preamble.name.clone()), record));
                    }
                    values.push((Name::from("action"),
                                 Record::NamedStruct(Name::Owned(action_name), param_values)));
                };
            },
            None => ()
        }

        if values.len() == 1 && table.is_nerpa_singleton() {
            Ok(values.pop().unwrap().1)
        } else {
            Ok(Record::NamedStruct(Name::Owned(table.base_name().into()), values))
        }
    }
}

fn parse_type_name(pnto: Option<&p4types::P4NamedType>) -> Option<String> {
    pnto.map(|pnt| pnt.name.clone())
}

/// Specification of the type for a parameter to an Action.
///
/// Based on the [P4Info `Param`
/// type](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action), which is not to be
/// confused with the [P4Runtime `Param`
/// type](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action-specification),
/// which provides the value for a parameter.
#[derive(Clone, Debug, Default)]
pub struct Param {
    /// Identification for this parameter.
    ///
    /// The protobuf representation of Param doesn't include a Preamble but it includes everything
    /// in the preamble except 'alias'.  It seems more uniform to just use Preamble here.
    pub preamble: Preamble,

    /// Width of the parameter's value in bits.
    pub bit_width: i32,

    /// Name of the parameter's type, if available.
    pub type_name: Option<String>,
}

impl Param {
    /// Returns the basic DDlog type to use for this `Param`, one of `"bit<N>"` or `"bool"`.
    pub fn p4_basic_type(&self) -> String {
        p4_basic_type(self.bit_width, &self.preamble.annotations)
    }

    /// Returns true if this `Param` should be represented in DDlog as a bool, false otherwise.
    pub fn is_nerpa_bool(&self) -> bool {
        is_nerpa_bool(self.bit_width, &self.preamble.annotations)
    }
}

impl From<&p4info::Action_Param> for Param {
    fn from(ap: &p4info::Action_Param) -> Self {
        Param {
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

impl Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: bit<{}>", self.preamble.name, self.bit_width)
    }
}

/// Specifies one kind of action that can be accepted in a [`Table`] via an intermediate
/// [`ActionRef`].
///
/// Based on P4Info [Action](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-action).
#[derive(Clone, Debug, Default)]
pub struct Action {
    /// Identification.
    pub preamble: Preamble,

    /// Parameters.
    pub params: Vec<Param>,
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

/// The scope within a [`Table`] in which a particular [`ActionRef`] may be used.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum Scope {
    /// Action may be used in a [`TableEntry`] or as the default action.
    TableAndDefault,

    /// Action may only be used in a [`TableEntry`].
    TableOnly,

    /// Action may only be used as the default action.
    DefaultOnly
}

impl Scope {
    /// Returns true iff an action with this `Scope` can be the default action in its table.
    pub fn may_be_default(self) -> bool {
        self != Self::TableOnly
    }

    /// Returns true iff an action with this `Scope` can appear as the action for an entry in its
    /// table.
    pub fn may_be_entry(self) -> bool {
        self != Self::DefaultOnly
    }
}

impl From<p4info::ActionRef_Scope> for Scope {
    fn from(scope: p4info::ActionRef_Scope) -> Self {
        match scope {
            p4info::ActionRef_Scope::TABLE_AND_DEFAULT => Self::TableAndDefault,
            p4info::ActionRef_Scope::TABLE_ONLY => Self::TableOnly,
            p4info::ActionRef_Scope::DEFAULT_ONLY => Self::DefaultOnly
        }
    }
}

/// Represents an action that may be used in a [`Table`].
///
/// Described within [this](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
#[derive(Clone, Debug)]
pub struct ActionRef {
    /// The action.
    pub action: Action,
    /// Is this action allowed in a table entry, as the default entry, or both?
    pub scope: Scope,
    /// Annotations for the action.
    pub annotations: Annotations,
}

impl ActionRef {
    /// Returns a new `ActionRef` based on `ar`.  The actions in the new `ActionRef` are looked up
    /// by ID in `actions` and cloned.
    pub fn new_from_proto(ar: &p4info::ActionRef, actions: &HashMap<u32, Action>) -> Self {
        ActionRef {
            action: actions.get(&ar.id).unwrap().clone(),
            scope: ar.scope.into(),
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
        if !self.scope.may_be_entry() {
            write!(f, "default-only ")?;
        } else if !self.scope.may_be_default() {
            write!(f, "not-default ")?;
        }
        write!(f, "{}", self.action)?;
        if !self.annotations.0.is_empty() {
            write!(f, " {}", self.annotations)?;
        };
        Ok(())
    }
}

/// Match-action table.
///
/// Based on [P4Runtime](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-table).
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
    /// Returns a new `Table` based on `t`.  The actions in the new `Table` are looked up by ID in
    /// `actions` and cloned.
    pub fn new_from_proto(t: &p4info::Table, actions: &HashMap<u32, Action>) -> Self {
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

    /// Extracts and returns the pipeline name from this `Table`.  (The P4 compiler names tables as
    /// `<pipeline>.<table>`.)  Returns None if the table's name isn't in the expected format.
    pub fn pipeline_name(&self) -> Option<&str> {
        match self.preamble.name.split('.').collect::<Vec<_>>().as_slice() {
            [pipeline_name, _table_name] => Some(pipeline_name),
            _ => None
        }
    }

    /// Extracts and returns the table name from this `Table`.  (The P4 compiler names tables as
    /// `<pipeline>.<table>`.)  Returns the table's full name if its name isn't in the expected
    /// format.
    pub fn base_name(&self) -> &str {
        match self.preamble.name.split('.').collect::<Vec<_>>().as_slice() {
            [_pipeline_name, table_name] => table_name,
            _ => self.preamble.name.as_str()
        }
    }

    /// Returns true if this table has a priority field, otherwise false.  Table entries have a
    /// priority field unless all of the match fields are exact-match.
    pub fn has_priority(&self) -> bool {
        self.match_fields.iter().any(|mf| mf.match_type != MatchType::Exact)
    }

    /// Returns only the actions that may be part of table entries, that is, actions with [`Scope`]
    /// of [`Scope::TableAndDefault`] or [`Scope::TableOnly`].
    pub fn entry_actions(&self) -> impl Iterator<Item=&ActionRef> {
        self.actions.iter().filter(|ar| ar.scope.may_be_entry())
    }

    /// Returns true if the user annotated this `Table` as one that should be represented as a
    /// DDlog singleton relation.  A singleton relation is declared in DDlog as holding a singleton
    /// type, e.g. `relation ReservedMcastDstDrop[bit<48>]`, as opposed to the more common
    /// named-struct kind of relation, e.g. `input relation FloodVlan(vlan: bit<12>)`.
    pub fn is_nerpa_singleton(&self) -> bool {
        self.preamble.annotations.0.contains_key("nerpa_singleton")
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
    let mut multicast_entry = proto::p4runtime::MulticastGroupEntry::new();
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
    let mut multicast_entry = proto::p4runtime::MulticastGroupEntry::new();
    multicast_entry.set_multicast_group_id(group_id);

    let mut pre_entry = PacketReplicationEngineEntry::new();
    pre_entry.set_multicast_group_entry(multicast_entry);

    let mut entity = proto::p4runtime::Entity::new();
    entity.set_packet_replication_engine_entry(pre_entry);

    entity
}
