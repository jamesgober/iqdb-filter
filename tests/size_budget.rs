//! Total-size caps on a filter: the depth and per-`In` caps alone leave a
//! wide `And` / `Or` unbounded.

#![allow(clippy::unwrap_used)]

use iqdb_filter::{
    FilterEvaluator, MAX_FILTER_DEPTH, MAX_FILTER_NODES, MAX_FILTER_VALUES, MAX_IN_VALUES,
};
use iqdb_types::{Filter, IqdbError, Value};

fn leaf(i: usize) -> Filter {
    Filter::eq("k", Value::Int(i as i64))
}

#[test]
fn wide_and_of_wide_in_nodes_is_rejected() {
    // Every child passes the per-`In` cap, and nothing used to bound how
    // many such children an `And` could hold.
    let wide_in = Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES]);
    let filter = Filter::And(vec![wide_in; 20]);
    assert_eq!(
        FilterEvaluator::new(filter).unwrap_err(),
        IqdbError::InvalidFilter
    );
    let narrow_many = Filter::And(vec![Filter::is_in("tag", vec![Value::Int(0)]); 10_000]);
    assert_eq!(
        FilterEvaluator::new(narrow_many).unwrap_err(),
        IqdbError::InvalidFilter
    );
}

#[test]
fn node_cap_is_exact() {
    // An `And` plus its children: exactly at the cap passes, one more fails.
    let at_cap = Filter::And((0..MAX_FILTER_NODES - 1).map(leaf).collect());
    assert!(FilterEvaluator::new(at_cap).is_ok());
    let over = Filter::And((0..MAX_FILTER_NODES).map(leaf).collect());
    assert_eq!(
        FilterEvaluator::new(over).unwrap_err(),
        IqdbError::InvalidFilter
    );
}

#[test]
fn node_cap_counts_nested_and_not_nodes() {
    // 64 groups of (Not + Or + 63 leaves) = 64 * 65 nodes, plus the root.
    let group = || Filter::Not(Box::new(Filter::Or((0..63).map(leaf).collect())));
    let filter = Filter::And((0..64).map(|_| group()).collect());
    assert_eq!(
        FilterEvaluator::new(filter).unwrap_err(),
        IqdbError::InvalidFilter
    );
    let smaller = Filter::And((0..60).map(|_| group()).collect());
    assert!(FilterEvaluator::new(smaller).is_ok());
}

#[test]
fn total_in_values_cap_is_exact() {
    let per = MAX_IN_VALUES;
    let n = MAX_FILTER_VALUES / per;
    let at_cap = Filter::Or(
        (0..n)
            .map(|_| Filter::is_in("tag", vec![Value::Int(1); per]))
            .collect(),
    );
    assert!(FilterEvaluator::new(at_cap).is_ok());
    let over = Filter::Or(
        (0..n)
            .map(|_| Filter::is_in("tag", vec![Value::Int(1); per]))
            .chain(std::iter::once(Filter::is_in("tag", vec![Value::Int(1)])))
            .collect(),
    );
    assert_eq!(
        FilterEvaluator::new(over).unwrap_err(),
        IqdbError::InvalidFilter
    );
}

#[test]
fn deep_filter_is_still_bounded_by_depth() {
    let mut f = leaf(0);
    for _ in 0..MAX_FILTER_DEPTH + 1 {
        f = Filter::Not(Box::new(f));
    }
    assert_eq!(
        FilterEvaluator::new(f).unwrap_err(),
        IqdbError::InvalidFilter
    );
}

#[test]
fn try_from_ref_validates_before_cloning() {
    let ok = Filter::And(vec![leaf(1), leaf(2)]);
    let evaluator = FilterEvaluator::try_from(&ok).unwrap();
    assert_eq!(evaluator.filter(), &ok);

    let too_wide = Filter::Or((0..MAX_FILTER_NODES).map(leaf).collect());
    assert_eq!(
        FilterEvaluator::try_from(&too_wide).unwrap_err(),
        IqdbError::InvalidFilter
    );
}
