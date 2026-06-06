//! Use a `MetadataIndex` to resolve a selective predicate to a candidate set,
//! then confirm exactness with the evaluator — the pre-filter pattern an index
//! crate uses to skip scanning every row.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example indexed_filtering
//! ```

use iqdb_filter::{FilterEvaluator, MetadataIndex};
use iqdb_types::{Filter, Metadata, Value};

fn record(pairs: &[(&str, Value)]) -> Metadata {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn main() -> iqdb_types::Result<()> {
    let corpus = [
        (
            0_usize,
            record(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2026)),
            ]),
        ),
        (
            1,
            record(&[
                ("lang", Value::String("go".into())),
                ("year", Value::Int(2024)),
            ]),
        ),
        (
            2,
            record(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2019)),
            ]),
        ),
        (
            3,
            record(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2026)),
            ]),
        ),
    ];

    // Opt in to indexing `lang` and `year`.
    let index = MetadataIndex::build(&["lang", "year"], corpus.iter().map(|(k, m)| (*k, Some(m))));
    println!("Indexed {} rows on fields: {:?}", index.len(), {
        let mut f: Vec<&str> = index.indexed_fields().collect();
        f.sort_unstable();
        f
    });

    // `lang == rust AND year > 2020`: the index resolves the equality, the range
    // is left to the evaluator.
    let filter = Filter::and(vec![
        Filter::eq("lang", Value::String("rust".into())),
        Filter::gt("year", Value::Int(2020)),
    ]);
    let evaluator = FilterEvaluator::new(filter)?;

    let metadata_of = |key: usize| corpus.iter().find(|(k, _)| *k == key).map(|(_, m)| m);

    let matches: Vec<usize> = match index.candidates(&evaluator) {
        Some(candidates) => {
            println!(
                "Index narrowed to {} candidates (of {}); confirming with evaluate.",
                candidates.len(),
                index.len()
            );
            candidates
                .into_iter()
                .filter(|&k| evaluator.evaluate(metadata_of(k)))
                .collect()
        }
        None => {
            println!("Index could not bound the predicate; full scan.");
            corpus
                .iter()
                .filter(|(_, m)| evaluator.evaluate(Some(m)))
                .map(|(k, _)| *k)
                .collect()
        }
    };

    let mut matches = matches;
    matches.sort_unstable();
    println!("\nMatches for `lang == rust AND year > 2020`: {matches:?}");
    assert_eq!(matches, [0, 3]);

    // Index-backed selectivity sees the true fraction: 3 of 4 rows are rust.
    let rust = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into())))?;
    println!(
        "\nIndex-backed selectivity of `lang == rust`: {:.2} (3 of 4 rows)",
        index.estimate_selectivity(&rust)
    );

    Ok(())
}
