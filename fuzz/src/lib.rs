//! Shared input generation for the `iqdb-filter` fuzz targets.
//!
//! `iqdb_types::{Filter, Value, Metadata}` are foreign types, so they cannot
//! derive [`arbitrary::Arbitrary`] here (orphan rule). Instead we derive it on
//! local `Spec*` mirrors and convert. Conversion caps nesting depth so an
//! adversarial input cannot stack-overflow the `Box<Filter>` drop while it is
//! being built — the validator rejects anything past `MAX_FILTER_DEPTH`
//! regardless.

use arbitrary::Arbitrary;
use iqdb_types::{Filter, Metadata, Value};

/// Nesting cap applied during conversion, comfortably above the evaluator's
/// `MAX_FILTER_DEPTH` (so the validator still does the real rejection) yet well
/// below anything that would overflow building or dropping the tree.
const MAX_CONVERT_DEPTH: usize = 96;

/// A fuzz-constructible mirror of [`iqdb_types::Value`].
#[derive(Arbitrary, Debug)]
pub enum SpecValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

impl SpecValue {
    fn to_value(&self) -> Value {
        match self {
            SpecValue::Str(s) => Value::String(s.clone()),
            SpecValue::Int(i) => Value::Int(*i),
            SpecValue::Float(f) => Value::Float(*f),
            SpecValue::Bool(b) => Value::Bool(*b),
            SpecValue::Null => Value::Null,
        }
    }
}

/// The comparison operator of a leaf predicate.
#[derive(Arbitrary, Debug)]
pub enum SpecOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
}

/// A fuzz-constructible mirror of [`iqdb_types::Filter`].
#[derive(Arbitrary, Debug)]
pub enum SpecFilter {
    Leaf(SpecOp, String, SpecValue),
    In(String, Vec<SpecValue>),
    And(Vec<SpecFilter>),
    Or(Vec<SpecFilter>),
    Not(Box<SpecFilter>),
}

/// The whole fuzz input: a filter, the fields to index, and a set of records.
#[derive(Arbitrary, Debug)]
pub struct FuzzInput {
    pub filter: SpecFilter,
    pub fields: Vec<String>,
    pub records: Vec<Vec<(String, SpecValue)>>,
}

/// Converts a [`SpecFilter`] into an [`iqdb_types::Filter`], capping depth.
#[must_use]
pub fn to_filter(spec: &SpecFilter) -> Filter {
    convert(spec, 0)
}

fn convert(spec: &SpecFilter, depth: usize) -> Filter {
    if depth >= MAX_CONVERT_DEPTH {
        // Collapse to a harmless leaf; the validator would reject this depth.
        return Filter::eq("__depth_capped__", Value::Null);
    }
    match spec {
        SpecFilter::Leaf(op, field, value) => {
            let v = value.to_value();
            match op {
                SpecOp::Eq => Filter::eq(field, v),
                SpecOp::Neq => Filter::neq(field, v),
                SpecOp::Lt => Filter::lt(field, v),
                SpecOp::Lte => Filter::lte(field, v),
                SpecOp::Gt => Filter::gt(field, v),
                SpecOp::Gte => Filter::gte(field, v),
            }
        }
        SpecFilter::In(field, values) => {
            Filter::is_in(field, values.iter().map(SpecValue::to_value).collect())
        }
        SpecFilter::And(children) => {
            Filter::and(children.iter().map(|c| convert(c, depth + 1)).collect())
        }
        SpecFilter::Or(children) => {
            Filter::or(children.iter().map(|c| convert(c, depth + 1)).collect())
        }
        SpecFilter::Not(inner) => Filter::not(convert(inner, depth + 1)),
    }
}

/// Converts the fuzzed records into [`Metadata`] values.
#[must_use]
pub fn to_records(records: &[Vec<(String, SpecValue)>]) -> Vec<Metadata> {
    records
        .iter()
        .map(|pairs| {
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), v.to_value()))
                .collect()
        })
        .collect()
}

/// Borrows the fuzzed field names as `&str` for [`iqdb_filter::MetadataIndex::build`].
#[must_use]
pub fn field_refs(fields: &[String]) -> Vec<&str> {
    fields.iter().map(String::as_str).collect()
}
