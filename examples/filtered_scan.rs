//! A mini "filtered scan": build one `FilterEvaluator`, then keep only the
//! records whose metadata matches — the exact shape `iqdb-flat` uses to
//! pre-filter candidates before scoring distances.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example filtered_scan
//! ```

use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};

fn record(pairs: &[(&str, Value)]) -> Metadata {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn main() -> iqdb_types::Result<()> {
    // published == true AND year > 2000 AND NOT archived
    let filter = Filter::and(vec![
        Filter::eq("published", Value::Bool(true)),
        Filter::gt("year", Value::Int(2000)),
        Filter::not(Filter::eq("archived", Value::Bool(true))),
    ]);

    // Validate once; reuse the evaluator for every row.
    let evaluator = FilterEvaluator::new(filter)?;

    let corpus = [
        (
            "doc-1",
            record(&[("published", Value::Bool(true)), ("year", Value::Int(2026))]),
        ),
        (
            "doc-2",
            record(&[("published", Value::Bool(true)), ("year", Value::Int(1998))]),
        ),
        (
            "doc-3",
            record(&[
                ("published", Value::Bool(false)),
                ("year", Value::Int(2024)),
            ]),
        ),
        (
            "doc-4",
            record(&[
                ("published", Value::Bool(true)),
                ("year", Value::Int(2025)),
                ("archived", Value::Bool(true)),
            ]),
        ),
    ];

    println!("Filter: published == true AND year > 2000 AND NOT archived\n");
    let mut kept = Vec::new();
    for (id, meta) in &corpus {
        let matched = evaluator.evaluate(Some(meta));
        println!("  {id}: {}", if matched { "KEEP" } else { "drop" });
        if matched {
            kept.push(*id);
        }
    }

    println!("\nSurvivors: {kept:?}");
    assert_eq!(kept, ["doc-1"]);
    Ok(())
}
