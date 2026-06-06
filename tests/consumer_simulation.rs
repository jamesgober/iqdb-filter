//! Consumer-simulation suite — the RC soak gate.
//!
//! A stand-in filtered searcher built **only** on the public `iqdb-filter`
//! surface, exercising it the way a real index crate (`iqdb-flat`, and later
//! `iqdb-hnsw` / `iqdb-ivf`) does: validate a filter once, narrow the candidate
//! set (via the `MetadataIndex` when it can bound the predicate, otherwise a
//! `prefilter` scan), confirm exactness with `evaluate`, then rank and take
//! top-`k`.
//!
//! The load-bearing assertion is **path equivalence**: the index-accelerated
//! search must return exactly what an unindexed full scan returns, for every
//! filter — that is the whole point of the index's superset contract. If the
//! two ever disagree, the index is dropping or inventing matches.

#![allow(clippy::unwrap_used)]

use iqdb_filter::{FilterEvaluator, FilterStrategy, MetadataIndex, StrategySelector};
use iqdb_types::{Filter, Metadata, Value};

/// A brute-force filtered searcher over rows of `(metadata, score)`. Ranking is
/// a stand-in for distance: smaller `|score - query|` is "nearer", with the row
/// index as a stable tie-breaker — the same shape as a real top-`k` loop.
struct SimIndex {
    metadata: Vec<Option<Metadata>>,
    scores: Vec<f64>,
    index: MetadataIndex<usize>,
}

impl SimIndex {
    fn new(indexed_fields: &[&str], rows: Vec<(Option<Metadata>, f64)>) -> Self {
        let metadata: Vec<Option<Metadata>> = rows.iter().map(|(m, _)| m.clone()).collect();
        let scores: Vec<f64> = rows.iter().map(|(_, s)| *s).collect();
        let index = MetadataIndex::build(
            indexed_fields,
            metadata.iter().enumerate().map(|(i, m)| (i, m.as_ref())),
        );
        Self {
            metadata,
            scores,
            index,
        }
    }

    fn metadata_of(&self, key: usize) -> Option<&Metadata> {
        self.metadata[key].as_ref()
    }

    /// Rank the given keys by nearness to `query` and keep the best `k`.
    fn rank(&self, mut keys: Vec<usize>, query: f64, k: usize) -> Vec<usize> {
        keys.sort_by(|&a, &b| {
            let da = (self.scores[a] - query).abs();
            let db = (self.scores[b] - query).abs();
            da.partial_cmp(&db).unwrap().then(a.cmp(&b))
        });
        keys.truncate(k);
        keys
    }

    /// Unindexed baseline: `prefilter` over every row, then rank.
    fn search_scan(&self, evaluator: &FilterEvaluator, query: f64, k: usize) -> Vec<usize> {
        let kept: Vec<usize> = evaluator
            .prefilter((0..self.metadata.len()).map(|i| (i, self.metadata_of(i))))
            .collect();
        self.rank(kept, query, k)
    }

    /// Index-accelerated: resolve candidates, confirm with `evaluate`, then
    /// rank. Falls back to a full scan when the index cannot bound the filter.
    fn search_indexed(&self, evaluator: &FilterEvaluator, query: f64, k: usize) -> Vec<usize> {
        let kept: Vec<usize> = match self.index.candidates(evaluator) {
            Some(candidates) => candidates
                .into_iter()
                .filter(|&key| evaluator.evaluate(self.metadata_of(key)))
                .collect(),
            None => evaluator
                .prefilter((0..self.metadata.len()).map(|i| (i, self.metadata_of(i))))
                .collect(),
        };
        self.rank(kept, query, k)
    }
}

fn meta(pairs: &[(&str, Value)]) -> Metadata {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn corpus() -> SimIndex {
    let rows = vec![
        (
            Some(meta(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2026)),
                ("published", Value::Bool(true)),
            ])),
            0.9,
        ),
        (
            Some(meta(&[
                ("lang", Value::String("go".into())),
                ("year", Value::Int(2024)),
                ("published", Value::Bool(true)),
            ])),
            0.5,
        ),
        (
            Some(meta(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2019)),
                ("published", Value::Bool(false)),
            ])),
            0.7,
        ),
        (
            Some(meta(&[
                ("lang", Value::String("rust".into())),
                ("year", Value::Int(2026)),
                ("published", Value::Bool(true)),
            ])),
            0.2,
        ),
        // A row with no metadata at all.
        (None, 0.4),
    ];
    SimIndex::new(&["lang", "year", "published"], rows)
}

/// Every filter shape must search identically through the index and a full scan.
fn assert_paths_agree(filter: Filter, query: f64, k: usize) {
    let index = corpus();
    let evaluator = FilterEvaluator::new(filter).unwrap();
    let scanned = index.search_scan(&evaluator, query, k);
    let indexed = index.search_indexed(&evaluator, query, k);
    assert_eq!(
        scanned, indexed,
        "index-accelerated search diverged from full scan"
    );
}

#[test]
fn indexed_equality_matches_scan() {
    assert_paths_agree(Filter::eq("lang", Value::String("rust".into())), 0.5, 10);
}

#[test]
fn compound_and_matches_scan() {
    assert_paths_agree(
        Filter::and(vec![
            Filter::eq("lang", Value::String("rust".into())),
            Filter::eq("published", Value::Bool(true)),
        ]),
        0.5,
        10,
    );
}

#[test]
fn unbounded_predicate_falls_back_but_agrees() {
    // A range the index cannot resolve: both paths must still match.
    assert_paths_agree(Filter::gt("year", Value::Int(2020)), 0.5, 10);
    // Negation, likewise unresolved by the index.
    assert_paths_agree(
        Filter::not(Filter::eq("lang", Value::String("rust".into()))),
        0.5,
        10,
    );
}

#[test]
fn mixed_resolvable_and_unresolvable_agrees() {
    // `lang == rust` (indexed) AND `year > 2020` (range, unresolved): the index
    // narrows on lang, the scan confirms the range — both must agree.
    assert_paths_agree(
        Filter::and(vec![
            Filter::eq("lang", Value::String("rust".into())),
            Filter::gt("year", Value::Int(2020)),
        ]),
        0.5,
        10,
    );
}

#[test]
fn truncates_to_k() {
    let index = corpus();
    let evaluator = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into()))).unwrap();
    let top1 = index.search_indexed(&evaluator, 0.0, 1);
    assert_eq!(top1.len(), 1);
    // Nearest to query 0.0 among rust rows (scores 0.9, 0.7, 0.2) is row 3.
    assert_eq!(top1, [3]);
}

#[test]
fn postfilter_refills_top_k_lazily() {
    // Post-filter form: hits arrive ranked, keep the first `k` that match.
    let index = corpus();
    let evaluator = FilterEvaluator::new(Filter::eq("published", Value::Bool(true))).unwrap();

    // Rank all rows by nearness to 0.5, then post-filter to published-only.
    let ranked = index.rank(
        (0..index.metadata.len()).collect(),
        0.5,
        index.metadata.len(),
    );
    let published: Vec<usize> = evaluator
        .postfilter(ranked.into_iter().map(|i| (i, index.metadata_of(i))))
        .take(2)
        .collect();

    // Exactly two published rows, and they are genuinely published.
    assert_eq!(published.len(), 2);
    for key in published {
        assert!(evaluator.evaluate(index.metadata_of(key)));
    }
}

#[test]
fn strategy_selection_drives_the_public_surface() {
    let index = corpus();
    let narrow = FilterEvaluator::new(Filter::eq("lang", Value::String("go".into()))).unwrap();

    // Structural and index-backed selectors both classify a 1-in-5 predicate
    // as narrow -> pre-filter.
    let selector = StrategySelector::new();
    assert_eq!(selector.choose(&narrow), FilterStrategy::PreFilter);
    assert_eq!(
        selector.choose_with_index(&narrow, &index.index),
        FilterStrategy::PreFilter
    );
}
