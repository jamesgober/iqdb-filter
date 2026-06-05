//! The closed-world rule in practice: how the evaluator treats fields a record
//! does not carry, and why `Neq(absent)` and `Not(Eq(absent))` give opposite
//! answers.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example absent_fields
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
    // Two filters that look equivalent but are not.
    let neq = FilterEvaluator::new(Filter::neq("author", Value::String("ada".into())))?;
    let not_eq = FilterEvaluator::new(Filter::not(Filter::eq(
        "author",
        Value::String("ada".into()),
    )))?;

    let no_author = record(&[("title", Value::String("untitled".into()))]);
    let by_grace = record(&[("author", Value::String("grace".into()))]);
    let by_ada = record(&[("author", Value::String("ada".into()))]);

    println!("                         Neq(author!=ada)   Not(Eq(author==ada))");
    for (label, meta) in [
        ("no author field   ", &no_author),
        ("author = grace     ", &by_grace),
        ("author = ada       ", &by_ada),
    ] {
        println!(
            "  {label}      {:<18} {}",
            neq.evaluate(Some(meta)),
            not_eq.evaluate(Some(meta)),
        );
    }

    // The key distinction: on a record with no `author`, `Neq` is false
    // (can't confirm an inequality without a value), but `Not(Eq)` is true
    // (the record genuinely is not authored by ada). This is the idiom for
    // "records missing this field, or carrying a non-matching value".
    assert!(!neq.evaluate(Some(&no_author)));
    assert!(not_eq.evaluate(Some(&no_author)));

    println!("\nOnly `Not(Eq)` selects the record that has no `author` field at all.");
    Ok(())
}
