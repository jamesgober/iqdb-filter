//! No-panic robustness for the validator and evaluator.
//!
//! Contract: [`FilterEvaluator::new`] either returns `Ok` or
//! `Err(IqdbError::InvalidFilter)` — never a panic, never a stack overflow,
//! regardless of how deep, wide, or hostile the filter is. On a validated
//! filter, [`evaluate`](iqdb_filter::FilterEvaluator::evaluate) and
//! [`prefilter`](iqdb_filter::FilterEvaluator::prefilter) must not panic for any
//! metadata, including `NaN`/`Inf` floats and absent fields.

#![no_main]

use std::hint::black_box;

use libfuzzer_sys::fuzz_target;

use iqdb_filter::FilterEvaluator;
use iqdb_filter_fuzz::{FuzzInput, to_filter, to_records};

fuzz_target!(|input: FuzzInput| {
    let filter = to_filter(&input.filter);

    // Validation must resolve to a typed result, never a panic.
    let Ok(evaluator) = FilterEvaluator::new(filter) else {
        return;
    };

    let records = to_records(&input.records);
    for metadata in &records {
        let _ = black_box(evaluator.evaluate(Some(metadata)));
    }
    let _ = black_box(evaluator.evaluate(None));

    // The scan helper must not panic either.
    let kept = evaluator
        .prefilter(records.iter().enumerate().map(|(i, m)| (i, Some(m))))
        .count();
    let _ = black_box(kept);
});
