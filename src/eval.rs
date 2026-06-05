//! The expression evaluator: walk a [`Filter`] tree against a record's
//! [`Metadata`] and return a boolean.
//!
//! This module is `pub(crate)`. Callers go through [`crate::FilterEvaluator`],
//! which validates the filter on construction and then borrows it into
//! [`eval`]. The semantics are intentionally strict and predictable, matching
//! the rules documented on [`iqdb_types::Filter`]:
//!
//! - **Absent field** — every leaf comparison (`Eq`, `Neq`, `Lt`, `Lte`,
//!   `Gt`, `Gte`, `In`) over a field the metadata does not carry evaluates
//!   to `false`. A `Neq` against an absent field is `false` too: we cannot
//!   confirm an inequality without a value to compare.
//! - **Type mismatch** — comparisons between values of different variants
//!   evaluate to `false`. Crossing types (an `Int` field vs a `String`
//!   literal) is treated as "does not match".
//! - **`Value::Float` with NaN** — ordered comparisons against NaN return
//!   `false` (NaN is not less than, equal to, or greater than anything under
//!   IEEE semantics).
//! - **Missing metadata entirely** — when the record has no metadata, every
//!   leaf evaluates to `false`. `Not` over a `false` leaf is `true`, which
//!   is the expected behaviour for "give me records without an `author`
//!   field".

use core::cmp::Ordering;

use iqdb_types::{Filter, Metadata, Value};

/// Evaluate `filter` against `metadata` (which may be absent).
///
/// The caller has already validated the filter via
/// [`crate::FilterEvaluator::new`]; depth is bounded by
/// [`crate::MAX_FILTER_DEPTH`], so the recursive descent here cannot exceed
/// that limit and the call stack stays bounded.
pub(crate) fn eval(filter: &Filter, metadata: Option<&Metadata>) -> bool {
    match filter {
        Filter::Eq { field, value } => match field_value(metadata, field) {
            Some(actual) => actual == value,
            None => false,
        },
        Filter::Neq { field, value } => match field_value(metadata, field) {
            Some(actual) => actual != value,
            None => false,
        },
        Filter::Lt { field, value } => {
            compare(field_value(metadata, field), value) == Some(Ordering::Less)
        }
        Filter::Lte { field, value } => matches!(
            compare(field_value(metadata, field), value),
            Some(Ordering::Less | Ordering::Equal)
        ),
        Filter::Gt { field, value } => {
            compare(field_value(metadata, field), value) == Some(Ordering::Greater)
        }
        Filter::Gte { field, value } => matches!(
            compare(field_value(metadata, field), value),
            Some(Ordering::Greater | Ordering::Equal)
        ),
        Filter::In { field, values } => match field_value(metadata, field) {
            Some(actual) => values.iter().any(|candidate| candidate == actual),
            None => false,
        },
        Filter::And(children) => children.iter().all(|child| eval(child, metadata)),
        Filter::Or(children) => children.iter().any(|child| eval(child, metadata)),
        Filter::Not(inner) => !eval(inner, metadata),
    }
}

fn field_value<'a>(metadata: Option<&'a Metadata>, field: &str) -> Option<&'a Value> {
    metadata?.get(field)
}

fn compare(actual: Option<&Value>, expected: &Value) -> Option<Ordering> {
    let actual = actual?;
    match (actual, expected) {
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn meta_with(pairs: &[(&str, Value)]) -> Metadata {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn eq_matches_present_field() {
        let meta = meta_with(&[("year", Value::Int(2026))]);
        assert!(eval(&Filter::eq("year", Value::Int(2026)), Some(&meta)));
    }

    #[test]
    fn eq_absent_field_is_false() {
        let meta = meta_with(&[]);
        assert!(!eval(&Filter::eq("year", Value::Int(2026)), Some(&meta)));
    }

    #[test]
    fn neq_absent_field_is_false() {
        let meta = meta_with(&[]);
        assert!(!eval(&Filter::neq("year", Value::Int(2026)), Some(&meta)));
    }

    #[test]
    fn gt_uses_int_ordering() {
        let meta = meta_with(&[("year", Value::Int(2026))]);
        assert!(eval(&Filter::gt("year", Value::Int(2000)), Some(&meta)));
        assert!(!eval(&Filter::gt("year", Value::Int(2030)), Some(&meta)));
    }

    #[test]
    fn type_mismatch_is_false() {
        let meta = meta_with(&[("year", Value::Int(2026))]);
        assert!(!eval(
            &Filter::eq("year", Value::String("2026".into())),
            Some(&meta)
        ));
        assert!(!eval(
            &Filter::lt("year", Value::String("9999".into())),
            Some(&meta)
        ));
    }

    #[test]
    fn nan_float_comparisons_are_false() {
        let meta = meta_with(&[("score", Value::Float(f64::NAN))]);
        assert!(!eval(&Filter::lt("score", Value::Float(1.0)), Some(&meta)));
        assert!(!eval(&Filter::gt("score", Value::Float(1.0)), Some(&meta)));
    }

    #[test]
    fn is_in_matches_any() {
        let meta = meta_with(&[("year", Value::Int(2026))]);
        assert!(eval(
            &Filter::is_in("year", vec![Value::Int(2025), Value::Int(2026)]),
            Some(&meta),
        ));
        assert!(!eval(
            &Filter::is_in("year", vec![Value::Int(2025)]),
            Some(&meta),
        ));
    }

    #[test]
    fn and_requires_all() {
        let meta = meta_with(&[("year", Value::Int(2026)), ("flag", Value::Bool(true))]);
        let filter = Filter::and(vec![
            Filter::eq("year", Value::Int(2026)),
            Filter::eq("flag", Value::Bool(true)),
        ]);
        assert!(eval(&filter, Some(&meta)));
    }

    #[test]
    fn or_requires_any() {
        let meta = meta_with(&[("year", Value::Int(2026))]);
        let filter = Filter::or(vec![
            Filter::eq("year", Value::Int(1999)),
            Filter::eq("year", Value::Int(2026)),
        ]);
        assert!(eval(&filter, Some(&meta)));
    }

    #[test]
    fn not_inverts_inner() {
        let meta = meta_with(&[("flag", Value::Bool(false))]);
        let filter = Filter::not(Filter::eq("flag", Value::Bool(true)));
        assert!(eval(&filter, Some(&meta)));
    }

    #[test]
    fn no_metadata_means_every_leaf_is_false() {
        let filter = Filter::eq("year", Value::Int(2026));
        assert!(!eval(&filter, None));
    }

    #[test]
    fn not_over_absent_field_is_true() {
        // The "records without an author" idiom.
        let filter = Filter::not(Filter::eq("author", Value::String("ada".into())));
        assert!(eval(&filter, None));
    }
}
