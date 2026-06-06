//! Let the crate recommend a strategy from a filter's estimated selectivity,
//! then apply the matching scan helper — `prefilter` for narrow predicates,
//! `postfilter` for broad ones.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example strategy_selection
//! ```

use iqdb_filter::{
    FilterEvaluator, FilterStrategy, StrategySelector, choose_strategy, estimate_selectivity,
};
use iqdb_types::{Filter, Metadata, Value};

fn record(pairs: &[(&str, Value)]) -> Metadata {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn report(label: &str, filter: Filter) -> iqdb_types::Result<()> {
    let evaluator = FilterEvaluator::new(filter)?;
    let selectivity = estimate_selectivity(&evaluator);
    let strategy = choose_strategy(&evaluator);
    println!("  {label:<22} selectivity ~{selectivity:.2}  ->  {strategy:?}");
    Ok(())
}

fn main() -> iqdb_types::Result<()> {
    println!("Default selector (threshold {:.2}):", 0.5);
    report("id == 42", Filter::eq("id", Value::Int(42)))?;
    report("id != 42", Filter::neq("id", Value::Int(42)))?;
    report(
        "published & recent",
        Filter::and(vec![
            Filter::eq("published", Value::Bool(true)),
            Filter::gt("year", Value::Int(2000)),
        ]),
    )?;

    // Apply the recommendation over a small corpus.
    let evaluator = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into())))?;
    let corpus = [
        (0_usize, record(&[("lang", Value::String("rust".into()))])),
        (1, record(&[("lang", Value::String("go".into()))])),
        (2, record(&[("lang", Value::String("rust".into()))])),
    ];

    let surviving: Vec<usize> = match choose_strategy(&evaluator) {
        FilterStrategy::PreFilter => evaluator
            .prefilter(corpus.iter().map(|(id, m)| (*id, Some(m))))
            .collect(),
        // For this exact-scan example the post-filter path keeps the same rows;
        // a real index would score first, then post-filter the ranked hits.
        _ => evaluator
            .postfilter(corpus.iter().map(|(id, m)| (*id, Some(m))))
            .collect(),
    };
    println!("\nRows matching `lang == rust`: {surviving:?}");
    assert_eq!(surviving, [0, 2]);

    // Tier-2: tune the cutoff. Forcing the threshold to 1.0 always pre-filters.
    let always_pre = StrategySelector::new().with_prefilter_threshold(1.0);
    let broad = FilterEvaluator::new(Filter::neq("id", Value::Int(42)))?;
    assert_eq!(always_pre.choose(&broad), FilterStrategy::PreFilter);
    println!("\nTuned selector (threshold 1.00) pre-filters even the broad `id != 42`.");

    Ok(())
}
