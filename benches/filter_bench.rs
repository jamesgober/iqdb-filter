//! Criterion micro-benchmarks for the two measurable paths in `iqdb-filter`:
//! one-time filter validation ([`FilterEvaluator::new`]) and the per-row
//! evaluation hot path ([`FilterEvaluator::evaluate`]) that runs inside a
//! search loop.
//!
//! Run with `cargo bench`. The evaluation benches are the ones that gate
//! releases — a >5% regression there is a blocker per `dev/DIRECTIVES.md`.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use iqdb_filter::{FilterEvaluator, MetadataIndex, choose_strategy, estimate_selectivity};
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

fn bench_strategy(c: &mut Criterion) {
    let evaluator =
        FilterEvaluator::new(representative_filter()).expect("representative filter is valid");

    let mut group = c.benchmark_group("strategy");
    group.bench_function("estimate_selectivity/compound", |b| {
        b.iter(|| black_box(estimate_selectivity(black_box(&evaluator))))
    });
    group.bench_function("choose_strategy/compound", |b| {
        b.iter(|| black_box(choose_strategy(black_box(&evaluator))))
    });
    group.finish();
}

fn bench_prefilter(c: &mut Criterion) {
    let evaluator =
        FilterEvaluator::new(representative_filter()).expect("representative filter is valid");

    // 1k rows, half carrying matching metadata.
    let rows: Vec<(usize, Option<Metadata>)> = (0..1000)
        .map(|i| {
            let meta = if i % 2 == 0 {
                matching_metadata()
            } else {
                [("published".to_string(), Value::Bool(false))]
                    .into_iter()
                    .collect()
            };
            (i, Some(meta))
        })
        .collect();

    c.bench_function("prefilter/1k_rows_50pct", |b| {
        b.iter(|| {
            let kept = evaluator
                .prefilter(rows.iter().map(|(k, m)| (*k, m.as_ref())))
                .count();
            black_box(kept)
        })
    });
}

fn bench_index(c: &mut Criterion) {
    // 1k rows over two indexed fields: `lang` (4 values) and `tier` (Int).
    let langs = ["rust", "go", "python", "zig"];
    let rows: Vec<(usize, Metadata)> = (0..1000)
        .map(|i| {
            let meta: Metadata = [
                (
                    "lang".to_string(),
                    Value::String(langs[i % langs.len()].into()),
                ),
                ("tier".to_string(), Value::Int((i % 5) as i64)),
            ]
            .into_iter()
            .collect();
            (i, meta)
        })
        .collect();

    let mut group = c.benchmark_group("index");
    group.bench_function("build/1k_rows_2_fields", |b| {
        b.iter(|| {
            let index =
                MetadataIndex::build(&["lang", "tier"], rows.iter().map(|(k, m)| (*k, Some(m))));
            black_box(index)
        })
    });

    let index = MetadataIndex::build(&["lang", "tier"], rows.iter().map(|(k, m)| (*k, Some(m))));
    let eq = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into())))
        .expect("valid filter");
    let and = FilterEvaluator::new(Filter::and(vec![
        Filter::eq("lang", Value::String("rust".into())),
        Filter::eq("tier", Value::Int(2)),
    ]))
    .expect("valid filter");

    group.bench_function("candidates/eq", |b| {
        b.iter(|| black_box(index.candidates(black_box(&eq))))
    });
    group.bench_function("candidates/and", |b| {
        b.iter(|| black_box(index.candidates(black_box(&and))))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_new,
    bench_evaluate,
    bench_strategy,
    bench_prefilter,
    bench_index
);
criterion_main!(benches);
