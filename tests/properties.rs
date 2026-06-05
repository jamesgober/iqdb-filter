//! Property-based coverage of the evaluator's core invariants
//! (`dev/DIRECTIVES.md` §5, §8). Where the unit and conformance tests pin
//! specific cases, these check that the algebraic laws hold across thousands
//! of randomly generated filters and records:
//!
//! - **Determinism** — `evaluate` is a pure function of `(filter, metadata)`.
//! - **Boolean algebra** — `Not`, `And`, `Or` compose exactly as logical
//!   negation, conjunction, and disjunction over the leaf results, including
//!   double-negation and De Morgan's laws.
//! - **Closed world** — every leaf over a field the record does not carry
//!   evaluates to `false`, and `Not` of such a leaf to `true`.

#![allow(clippy::unwrap_used)]

use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};
use proptest::prelude::*;

// Small, deliberately-overlapping domains so generated filters and metadata
// frequently reference the same fields and values — the interesting cases.
const FIELDS: &[&str] = &["a", "b", "c", "d"];

// A field name the generators never produce, so it is guaranteed absent from
// any generated `Metadata` — used to exercise the closed-world rule.
const ABSENT_FIELD: &str = "never_generated_field";

fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        (0..5i64).prop_map(Value::Int),
        any::<bool>().prop_map(Value::Bool),
        // Finite floats only: NaN ordering is covered explicitly elsewhere and
        // would break the reflexive identities these properties assert.
        (-5.0f64..5.0).prop_map(Value::Float),
        (0usize..4).prop_map(|i| Value::String(format!("v{i}"))),
        Just(Value::Null),
    ]
}

fn arb_field() -> impl Strategy<Value = String> {
    (0usize..FIELDS.len()).prop_map(|i| FIELDS[i].to_string())
}

fn arb_leaf() -> impl Strategy<Value = Filter> {
    prop_oneof![
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::eq(f, v)),
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::neq(f, v)),
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::lt(f, v)),
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::lte(f, v)),
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::gt(f, v)),
        (arb_field(), arb_value()).prop_map(|(f, v)| Filter::gte(f, v)),
        (arb_field(), prop::collection::vec(arb_value(), 0..6))
            .prop_map(|(f, vs)| Filter::is_in(f, vs)),
    ]
}

fn arb_filter() -> impl Strategy<Value = Filter> {
    arb_leaf().prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(Filter::not),
            prop::collection::vec(inner.clone(), 1..4).prop_map(Filter::and),
            prop::collection::vec(inner, 1..4).prop_map(Filter::or),
        ]
    })
}

fn arb_metadata() -> impl Strategy<Value = Metadata> {
    prop::collection::vec((arb_field(), arb_value()), 0..6)
        .prop_map(|pairs| pairs.into_iter().collect())
}

fn eval(filter: Filter, meta: &Metadata) -> bool {
    FilterEvaluator::new(filter).unwrap().evaluate(Some(meta))
}

proptest! {
    #[test]
    fn evaluate_is_deterministic(filter in arb_filter(), meta in arb_metadata()) {
        let e = FilterEvaluator::new(filter).unwrap();
        prop_assert_eq!(e.evaluate(Some(&meta)), e.evaluate(Some(&meta)));
    }

    #[test]
    fn not_negates(filter in arb_filter(), meta in arb_metadata()) {
        let base = eval(filter.clone(), &meta);
        let negated = eval(Filter::not(filter), &meta);
        prop_assert_eq!(negated, !base);
    }

    #[test]
    fn double_negation_is_identity(filter in arb_filter(), meta in arb_metadata()) {
        let base = eval(filter.clone(), &meta);
        let twice = eval(Filter::not(Filter::not(filter)), &meta);
        prop_assert_eq!(twice, base);
    }

    #[test]
    fn and_is_conjunction(a in arb_filter(), b in arb_filter(), meta in arb_metadata()) {
        let ea = eval(a.clone(), &meta);
        let eb = eval(b.clone(), &meta);
        let conj = eval(Filter::and(vec![a, b]), &meta);
        prop_assert_eq!(conj, ea && eb);
    }

    #[test]
    fn or_is_disjunction(a in arb_filter(), b in arb_filter(), meta in arb_metadata()) {
        let ea = eval(a.clone(), &meta);
        let eb = eval(b.clone(), &meta);
        let disj = eval(Filter::or(vec![a, b]), &meta);
        prop_assert_eq!(disj, ea || eb);
    }

    #[test]
    fn de_morgan(a in arb_filter(), b in arb_filter(), meta in arb_metadata()) {
        let lhs = eval(Filter::not(Filter::and(vec![a.clone(), b.clone()])), &meta);
        let rhs = eval(Filter::or(vec![Filter::not(a), Filter::not(b)]), &meta);
        prop_assert_eq!(lhs, rhs);
    }

    #[test]
    fn leaf_over_absent_field_is_false(value in arb_value(), meta in arb_metadata()) {
        // No generated metadata ever carries `ABSENT_FIELD`, so every leaf
        // over it must be false — and `Not` of it true.
        let leaf = Filter::eq(ABSENT_FIELD, value.clone());
        prop_assert!(!eval(leaf.clone(), &meta));
        prop_assert!(eval(Filter::not(leaf), &meta));

        let neq = Filter::neq(ABSENT_FIELD, value.clone());
        prop_assert!(!eval(neq, &meta));

        let in_set = Filter::is_in(ABSENT_FIELD, vec![value]);
        prop_assert!(!eval(in_set, &meta));
    }
}
