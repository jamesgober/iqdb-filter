//! Cross-cutting contracts pinned as integration tests so they survive
//! refactors of either the evaluator or `iqdb-types::Filter`:
//!
//! - **Null / absent-field semantics** — the closed-world rule, including the
//!   non-interchangeable `Neq(absent)` vs `Not(Eq(absent))` pair.
//! - **Depth cap** — validation rejects over-deep filters without overflowing.
//! - **`In` cardinality cap** — oversized membership predicates are rejected
//!   wherever the node appears in the tree.
//!
//! Unit-level evaluation rules are covered inside the modules; this file pins
//! the public, cross-module guarantees.

#![allow(clippy::unwrap_used)]

use iqdb_filter::{FilterEvaluator, MAX_FILTER_DEPTH, MAX_IN_VALUES};
use iqdb_types::{Filter, IqdbError, Metadata, Value};

fn meta_with(pairs: &[(&str, Value)]) -> Metadata {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn nested_not(depth: usize) -> Filter {
    let mut current = Filter::eq("k", Value::Int(0));
    for _ in 0..depth {
        current = Filter::not(current);
    }
    current
}

// ---- Closed-world null / absent-field semantics -------------------------

#[test]
fn neq_absent_is_false() {
    let evaluator =
        FilterEvaluator::new(Filter::neq("author", Value::String("ada".into()))).unwrap();
    let empty_meta = meta_with(&[]);

    // Neq over an absent field: false. The library refuses to confirm an
    // inequality without a value to compare.
    assert!(!evaluator.evaluate(Some(&empty_meta)));
    assert!(!evaluator.evaluate(None));
}

#[test]
fn not_eq_absent_is_true() {
    let evaluator = FilterEvaluator::new(Filter::not(Filter::eq(
        "author",
        Value::String("ada".into()),
    )))
    .unwrap();
    let empty_meta = meta_with(&[]);

    // Not(Eq) over an absent field: true. This is the idiom for "records
    // that do not have this field, or have it with a non-matching value".
    assert!(evaluator.evaluate(Some(&empty_meta)));
    assert!(evaluator.evaluate(None));
}

#[test]
fn neq_present_field_matches() {
    let evaluator =
        FilterEvaluator::new(Filter::neq("author", Value::String("ada".into()))).unwrap();
    let meta = meta_with(&[("author", Value::String("grace".into()))]);
    assert!(evaluator.evaluate(Some(&meta)));
}

#[test]
fn type_mismatch_is_false() {
    let evaluator = FilterEvaluator::new(Filter::eq("year", Value::String("2026".into()))).unwrap();
    let meta = meta_with(&[("year", Value::Int(2026))]);
    assert!(!evaluator.evaluate(Some(&meta)));
}

// ---- Depth cap: validation must not overflow ----------------------------

#[test]
fn depth_at_cap_accepted() {
    let filter = nested_not(MAX_FILTER_DEPTH);
    assert!(FilterEvaluator::new(filter).is_ok());
}

#[test]
fn depth_over_cap_rejected() {
    let filter = nested_not(MAX_FILTER_DEPTH + 1);
    let err = FilterEvaluator::new(filter).unwrap_err();
    assert_eq!(err, IqdbError::InvalidFilter);
}

#[test]
fn validation_does_not_overflow_on_pathological_input() {
    // Far past the cap. The validation walk is iterative, so even an
    // adversarial-shaped filter yields a clean Err — not a stack overflow.
    // Building the nested tree itself uses recursion in the Drop impl of
    // `Box<Filter>`; cap depth at a level Drop can comfortably unwind on the
    // test thread stack (much smaller than what would overflow `validate`).
    let filter = nested_not(MAX_FILTER_DEPTH * 4);
    let err = FilterEvaluator::new(filter).unwrap_err();
    assert_eq!(err, IqdbError::InvalidFilter);
}

#[test]
fn wide_and_chain_at_cap_accepted() {
    // Same depth budget, but built with `And` instead of `Not` to exercise
    // the children-loop branch of the validator.
    let mut current = Filter::eq("k", Value::Int(0));
    for _ in 0..MAX_FILTER_DEPTH {
        current = Filter::and(vec![current]);
    }
    assert!(FilterEvaluator::new(current).is_ok());
}

// ---- `In` cardinality cap -----------------------------------------------

#[test]
fn in_at_cap_accepted() {
    let filter = Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES]);
    assert!(FilterEvaluator::new(filter).is_ok());
}

#[test]
fn in_over_cap_rejected() {
    let filter = Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES + 1]);
    let err = FilterEvaluator::new(filter).unwrap_err();
    assert_eq!(err, IqdbError::InvalidFilter);
}

#[test]
fn in_inside_and_still_rejected() {
    // The In cap is enforced wherever the node appears, not only at the root.
    let filter = Filter::and(vec![
        Filter::eq("k", Value::Int(1)),
        Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES + 1]),
    ]);
    let err = FilterEvaluator::new(filter).unwrap_err();
    assert_eq!(err, IqdbError::InvalidFilter);
}

// ---- Sanity --------------------------------------------------------------

#[test]
fn evaluate_round_trips_a_real_query() {
    let filter = Filter::and(vec![
        Filter::eq("published", Value::Bool(true)),
        Filter::or(vec![
            Filter::gt("year", Value::Int(2000)),
            Filter::eq("year", Value::Int(2000)),
        ]),
    ]);
    let evaluator = FilterEvaluator::new(filter).unwrap();

    let matching = meta_with(&[("published", Value::Bool(true)), ("year", Value::Int(2026))]);
    let mismatch = meta_with(&[("published", Value::Bool(true)), ("year", Value::Int(1999))]);

    assert!(evaluator.evaluate(Some(&matching)));
    assert!(!evaluator.evaluate(Some(&mismatch)));
}
