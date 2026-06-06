//! The `MetadataIndex` superset contract, fuzzed.
//!
//! Contract: when [`MetadataIndex::candidates`] returns `Some(set)`, every
//! record the evaluator accepts is in `set` (false positives allowed, false
//! negatives never). This target builds an arbitrary index over arbitrary
//! records and asserts that invariant for an arbitrary filter — the property
//! `tests/properties.rs` checks on bounded inputs, here on unbounded ones.

#![no_main]

use std::collections::HashSet;

use libfuzzer_sys::fuzz_target;

use iqdb_filter::{FilterEvaluator, MetadataIndex};
use iqdb_filter_fuzz::{FuzzInput, field_refs, to_filter, to_records};

fuzz_target!(|input: FuzzInput| {
    let Ok(evaluator) = FilterEvaluator::new(to_filter(&input.filter)) else {
        return;
    };

    let records = to_records(&input.records);
    let fields = field_refs(&input.fields);
    let index = MetadataIndex::build(
        &fields,
        records.iter().enumerate().map(|(i, m)| (i, Some(m))),
    );

    if let Some(candidates) = index.candidates(&evaluator) {
        let candidate_set: HashSet<usize> = candidates.into_iter().collect();
        for (i, metadata) in records.iter().enumerate() {
            if evaluator.evaluate(Some(metadata)) {
                assert!(
                    candidate_set.contains(&i),
                    "superset contract violated: true match {i} absent from candidates"
                );
            }
        }
    }
});
