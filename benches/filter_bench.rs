//! Criterion micro-benchmarks for the two measurable paths in `iqdb-filter`:
//! one-time filter validation ([`FilterEvaluator::new`]) and the per-row
//! evaluation hot path ([`FilterEvaluator::evaluate`]) that runs inside a
//! search loop.
//!
//! Run with `cargo bench`. The evaluation benches are the ones that gate
//! releases — a >5% regression there is a blocker per `dev/DIRECTIVES.md`.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};

/// A representative compound predicate: `published AND (year > 2000 OR
/// genre IN {…}) AND NOT archived`. Shaped like a real query so the numbers
/// reflect production cost, not a degenerate single leaf.
fn representative_filter() -> Filter {
    Filter::and(vec![
        Filter::eq("published", Value::Bool(true)),
        Filter::or(vec![
            Filter::gt("year", Value::Int(2000)),
            Filter::is_in(
                "genre",
                vec![
                    Value::String("rust".into()),
                    Value::String("systems".into()),
                    Value::String("databases".into()),
                ],
            ),
        ]),
        Filter::not(Filter::eq("archived", Value::Bool(true))),
    ])
}

fn matching_metadata() -> Metadata {
    [
        ("published".to_string(), Value::Bool(true)),
        ("year".to_string(), Value::Int(2026)),
        ("genre".to_string(), Value::String("databases".into())),
        ("title".to_string(), Value::String("iqdb".into())),
    ]
    .into_iter()
    .collect()
}

fn bench_new(c: &mut Criterion) {
    c.bench_function("evaluator_new/compound", |b| {
        b.iter(|| {
            let evaluator = FilterEvaluator::new(black_box(representative_filter()));
            let _ = black_box(evaluator);
        })
    });
}

fn bench_evaluate(c: &mut Criterion) {
    let evaluator =
        FilterEvaluator::new(representative_filter()).expect("representative filter is valid");
    let meta = matching_metadata();

    let mut group = c.benchmark_group("evaluate");
    group.bench_function("compound/match", |b| {
        b.iter(|| black_box(evaluator.evaluate(black_box(Some(&meta)))))
    });
    group.bench_function("compound/no_metadata", |b| {
        b.iter(|| black_box(evaluator.evaluate(black_box(None))))
    });
    group.finish();
}

criterion_group!(benches, bench_new, bench_evaluate);
criterion_main!(benches);
