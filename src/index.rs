//! [`MetadataIndex`] — an opt-in, per-field inverted index that turns a
//! selective predicate into a candidate key set without scanning every row.
//!
//! # What it is
//!
//! For each **explicitly indexed** field, the index keeps an inverted map from
//! a metadata value to the keys of the records that carry it. A consumer hands
//! a validated filter to [`candidates`](MetadataIndex::candidates) and gets back
//! the keys that *might* match — a **superset** of the true matches — then
//! re-runs [`FilterEvaluator::evaluate`](crate::FilterEvaluator::evaluate) on
//! that smaller set for an exact answer. The win is skipping the per-row test
//! (and, for an approximate index, the distance computation) on the rows the
//! predicate already excludes.
//!
//! # What it resolves
//!
//! The index resolves the predicates an inverted index is good at — equality
//! and membership — over the value types with well-behaved equality:
//!
//! - [`Filter::Eq`] / [`Filter::In`] on an **indexed** field whose literal is a
//!   `String`, `Int`, `Bool`, or `Null`.
//! - [`Filter::And`] — the intersection of whatever children resolve (any
//!   unresolved child is left to `evaluate`).
//! - [`Filter::Or`] — the union, but **only if every child resolves** (an
//!   unresolved branch could match anything).
//!
//! Everything else returns [`None`] from [`candidates`](MetadataIndex::candidates),
//! meaning "I can't bound this — scan everything": ranges
//! (`Lt`/`Lte`/`Gt`/`Gte`), [`Filter::Not`], `Float` literals (IEEE equality
//! makes a float a poor index key, and missing one would drop a real match),
//! and any field that was not indexed.
//!
//! # Correctness contract
//!
//! Whenever [`candidates`](MetadataIndex::candidates) returns `Some(set)`, every
//! record the evaluator would accept is in `set`. False positives are allowed
//! (the consumer filters them with `evaluate`); false negatives never happen.
//!
//! # Why opt-in per field
//!
//! Indexing a field costs memory and per-insert work. Most metadata fields are
//! never filtered on, so the index only tracks the fields a caller names.
//!
//! # Example
//!
//! ```
//! use iqdb_filter::{FilterEvaluator, MetadataIndex};
//! use iqdb_types::{Filter, Metadata, Value};
//!
//! # fn main() -> iqdb_types::Result<()> {
//! fn meta(pairs: &[(&str, Value)]) -> Metadata {
//!     pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
//! }
//!
//! let rows = [
//!     (0_usize, meta(&[("lang", Value::String("rust".into()))])),
//!     (1, meta(&[("lang", Value::String("go".into()))])),
//!     (2, meta(&[("lang", Value::String("rust".into()))])),
//! ];
//!
//! // Index only the `lang` field.
//! let index = MetadataIndex::build(&["lang"], rows.iter().map(|(k, m)| (*k, Some(m))));
//!
//! let evaluator = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into())))?;
//! let mut candidates = index.candidates(&evaluator).expect("indexed field resolves");
//! candidates.sort_unstable();
//! assert_eq!(candidates, [0, 2]);
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use iqdb_types::{Filter, Metadata, Value};

use crate::FilterEvaluator;
use crate::selectivity;

/// The subset of [`Value`] variants the index can use as a posting-list key:
/// everything except `Float`, whose IEEE-754 equality (`NaN != NaN`,
/// `+0.0 == -0.0`) does not survive a hash-map round-trip without risking a
/// dropped match.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IndexKey {
    Str(String),
    Int(i64),
    Bool(bool),
    Null,
}

impl IndexKey {
    /// Returns the index key for `value`, or `None` for a `Float` (which the
    /// index deliberately does not key on).
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(IndexKey::Str(s.clone())),
            Value::Int(i) => Some(IndexKey::Int(*i)),
            Value::Bool(b) => Some(IndexKey::Bool(*b)),
            Value::Null => Some(IndexKey::Null),
            Value::Float(_) => None,
        }
    }
}

/// An opt-in, per-field inverted index over record metadata.
///
/// `K` is the caller's row key — a storage index, an [`iqdb_types::VectorId`],
/// or any `Clone + Eq + Hash` handle. Build one with
/// [`build`](MetadataIndex::build); it is immutable thereafter (rebuild to
/// reflect new data). See [`candidates`](MetadataIndex::candidates) for the
/// resolution rules and the superset contract.
#[derive(Debug, Clone)]
pub struct MetadataIndex<K> {
    /// indexed field -> (value key -> the rows carrying it)
    fields: HashMap<String, HashMap<IndexKey, Vec<K>>>,
    /// Total records seen at build time (the denominator for selectivity).
    total: usize,
}

impl<K> MetadataIndex<K>
where
    K: Clone + Eq + Hash,
{
    /// Builds an index over `fields` from `records`.
    ///
    /// `fields` is the explicit opt-in list — only these fields are indexed.
    /// `records` yields `(key, metadata)` pairs; a record with `None` metadata
    /// (or one missing an indexed field) still counts toward the total but
    /// contributes no postings. A field named in `fields` that no record
    /// carries simply has an empty posting set.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::MetadataIndex;
    /// use iqdb_types::{Metadata, Value};
    ///
    /// let rows = [(0_u64, [("k".to_string(), Value::Int(1))].into_iter().collect::<Metadata>())];
    /// let index = MetadataIndex::build(&["k"], rows.iter().map(|(k, m)| (*k, Some(m))));
    /// assert_eq!(index.len(), 1);
    /// assert!(index.is_indexed("k"));
    /// ```
    pub fn build<'a, I>(fields: &[&str], records: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<&'a Metadata>)>,
    {
        let mut field_maps: HashMap<String, HashMap<IndexKey, Vec<K>>> = fields
            .iter()
            .map(|f| ((*f).to_string(), HashMap::new()))
            .collect();

        let mut total = 0usize;
        for (key, metadata) in records {
            total += 1;
            let Some(metadata) = metadata else { continue };
            for (field, postings) in field_maps.iter_mut() {
                let Some(value) = metadata.get(field) else {
                    continue;
                };
                let Some(index_key) = IndexKey::from_value(value) else {
                    continue;
                };
                postings.entry(index_key).or_default().push(key.clone());
            }
        }

        Self {
            fields: field_maps,
            total,
        }
    }

    /// The number of records the index was built from.
    #[must_use]
    pub fn len(&self) -> usize {
        self.total
    }

    /// Whether the index was built from zero records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Whether `field` is one of the indexed fields.
    #[must_use]
    pub fn is_indexed(&self, field: &str) -> bool {
        self.fields.contains_key(field)
    }

    /// The set of indexed field names, in unspecified order.
    pub fn indexed_fields(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }

    /// Resolves `evaluator`'s filter to a candidate key set, or `None` if the
    /// index cannot bound it (scan everything in that case).
    ///
    /// When this returns `Some(keys)`, `keys` is a **superset** of the records
    /// the evaluator accepts — re-run
    /// [`evaluate`](crate::FilterEvaluator::evaluate) over them for the exact
    /// result. The keys are unique and in unspecified order.
    ///
    /// Takes a validated [`FilterEvaluator`], so the recursive walk is bounded
    /// by [`MAX_FILTER_DEPTH`](crate::MAX_FILTER_DEPTH).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::{FilterEvaluator, MetadataIndex};
    /// use iqdb_types::{Filter, Metadata, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let rows = [
    ///     (0_usize, [("tier".to_string(), Value::Int(1))].into_iter().collect::<Metadata>()),
    ///     (1, [("tier".to_string(), Value::Int(2))].into_iter().collect::<Metadata>()),
    /// ];
    /// let index = MetadataIndex::build(&["tier"], rows.iter().map(|(k, m)| (*k, Some(m))));
    ///
    /// // Indexed equality resolves.
    /// let eq = FilterEvaluator::new(Filter::eq("tier", Value::Int(1)))?;
    /// assert_eq!(index.candidates(&eq), Some(vec![0]));
    ///
    /// // A range over an indexed field is left to a full scan.
    /// let range = FilterEvaluator::new(Filter::gt("tier", Value::Int(1)))?;
    /// assert_eq!(index.candidates(&range), None);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn candidates(&self, evaluator: &FilterEvaluator) -> Option<Vec<K>> {
        self.resolve(evaluator.filter())
            .map(|set| set.into_iter().collect())
    }

    /// Returns the candidate set for `filter`, or `None` when it cannot be
    /// bounded by the index.
    fn resolve(&self, filter: &Filter) -> Option<HashSet<K>> {
        match filter {
            Filter::Eq { field, value } => self.postings_for(field, value),
            Filter::In { field, values } => {
                let mut acc: HashSet<K> = HashSet::new();
                for value in values {
                    // Any non-indexable member makes the union unbounded.
                    acc.extend(self.postings_for(field, value)?);
                }
                Some(acc)
            }
            Filter::And(children) => {
                // Intersect the children that resolve; an unresolved child is
                // left to `evaluate`. The intersection of resolvable
                // constraints is still a superset of the true matches.
                let mut acc: Option<HashSet<K>> = None;
                for child in children {
                    if let Some(set) = self.resolve(child) {
                        acc = Some(match acc {
                            None => set,
                            Some(current) => intersect(current, &set),
                        });
                    }
                }
                acc
            }
            Filter::Or(children) => {
                // A union is only bounded if every branch is.
                let mut acc: HashSet<K> = HashSet::new();
                for child in children {
                    acc.extend(self.resolve(child)?);
                }
                Some(acc)
            }
            // Negation, inequality, and ranges are anti-selective or
            // non-equality: the index cannot bound them, so the caller scans.
            Filter::Neq { .. }
            | Filter::Not(_)
            | Filter::Lt { .. }
            | Filter::Lte { .. }
            | Filter::Gt { .. }
            | Filter::Gte { .. } => None,
        }
    }

    /// The posting set for `field == value`, or `None` if the field is not
    /// indexed or the value is not an index key (a `Float`). A `Some(empty)`
    /// means the field is indexed but nothing carries that value.
    fn postings_for(&self, field: &str, value: &Value) -> Option<HashSet<K>> {
        let postings = self.fields.get(field)?;
        let index_key = IndexKey::from_value(value)?;
        Some(
            postings
                .get(&index_key)
                .map(|keys| keys.iter().cloned().collect())
                .unwrap_or_default(),
        )
    }

    /// Estimates the fraction of records `evaluator`'s filter passes, in
    /// `[0.0, 1.0]`, using real posting counts where the index can and the
    /// structural [`estimate_selectivity`](crate::estimate_selectivity)
    /// fallback elsewhere.
    ///
    /// This is the data-backed counterpart to the structural estimate: an
    /// indexed `Eq` / `In` leaf contributes its actual `matches / total`
    /// fraction, so a selector backed by the index makes sharper pre/post
    /// decisions. With zero records it falls back entirely to the structural
    /// estimate.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::{FilterEvaluator, MetadataIndex};
    /// use iqdb_types::{Filter, Metadata, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// // 1 of 4 rows has status == 1: the index knows the true 0.25.
    /// let rows = [
    ///     (0_usize, [("status".to_string(), Value::Int(1))].into_iter().collect::<Metadata>()),
    ///     (1, [("status".to_string(), Value::Int(2))].into_iter().collect::<Metadata>()),
    ///     (2, [("status".to_string(), Value::Int(2))].into_iter().collect::<Metadata>()),
    ///     (3, [("status".to_string(), Value::Int(3))].into_iter().collect::<Metadata>()),
    /// ];
    /// let index = MetadataIndex::build(&["status"], rows.iter().map(|(k, m)| (*k, Some(m))));
    ///
    /// let eq = FilterEvaluator::new(Filter::eq("status", Value::Int(1)))?;
    /// assert!((index.estimate_selectivity(&eq) - 0.25).abs() < 1e-9);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn estimate_selectivity(&self, evaluator: &FilterEvaluator) -> f64 {
        if self.total == 0 {
            return selectivity::structural(evaluator.filter()).clamp(0.0, 1.0);
        }
        self.estimate(evaluator.filter()).clamp(0.0, 1.0)
    }

    fn estimate(&self, filter: &Filter) -> f64 {
        match filter {
            Filter::Eq { field, value } => self
                .leaf_fraction(field, value)
                .unwrap_or(selectivity::EQ_SELECTIVITY),
            Filter::Neq { field, value } => {
                1.0 - self
                    .leaf_fraction(field, value)
                    .unwrap_or(selectivity::EQ_SELECTIVITY)
            }
            Filter::In { field, values } => {
                // Sum the per-value fractions when every member is indexable;
                // otherwise fall back to the structural estimate for the node.
                let mut acc = 0.0;
                for value in values {
                    match self.leaf_fraction(field, value) {
                        Some(f) => acc += f,
                        None => return selectivity::structural(filter),
                    }
                }
                acc.min(1.0)
            }
            Filter::Lt { .. } | Filter::Lte { .. } | Filter::Gt { .. } | Filter::Gte { .. } => {
                selectivity::RANGE_SELECTIVITY
            }
            Filter::And(children) => children.iter().map(|c| self.estimate(c)).product(),
            Filter::Or(children) => {
                1.0 - children
                    .iter()
                    .map(|c| 1.0 - self.estimate(c))
                    .product::<f64>()
            }
            Filter::Not(inner) => 1.0 - self.estimate(inner),
        }
    }

    /// The real `matches / total` fraction for `field == value`, or `None` when
    /// the field is not indexed or the value is not an index key.
    fn leaf_fraction(&self, field: &str, value: &Value) -> Option<f64> {
        let postings = self.fields.get(field)?;
        let index_key = IndexKey::from_value(value)?;
        let count = postings.get(&index_key).map_or(0, Vec::len);
        Some(count as f64 / self.total as f64)
    }
}

/// Intersection that retains the elements of `a` also present in `b`.
fn intersect<K: Eq + Hash>(a: HashSet<K>, b: &HashSet<K>) -> HashSet<K> {
    a.into_iter().filter(|k| b.contains(k)).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use iqdb_types::Metadata;

    fn meta(pairs: &[(&str, Value)]) -> Metadata {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn corpus() -> Vec<(usize, Metadata)> {
        vec![
            (
                0,
                meta(&[
                    ("lang", Value::String("rust".into())),
                    ("year", Value::Int(2026)),
                ]),
            ),
            (
                1,
                meta(&[
                    ("lang", Value::String("go".into())),
                    ("year", Value::Int(2024)),
                ]),
            ),
            (
                2,
                meta(&[
                    ("lang", Value::String("rust".into())),
                    ("year", Value::Int(2020)),
                ]),
            ),
            (3, meta(&[("lang", Value::String("rust".into()))])),
        ]
    }

    fn index() -> MetadataIndex<usize> {
        let rows = corpus();
        MetadataIndex::build(&["lang", "year"], rows.iter().map(|(k, m)| (*k, Some(m))))
    }

    fn sorted(mut v: Vec<usize>) -> Vec<usize> {
        v.sort_unstable();
        v
    }

    fn cands(filter: Filter) -> Option<Vec<usize>> {
        let evaluator = FilterEvaluator::new(filter).unwrap();
        index().candidates(&evaluator).map(sorted)
    }

    #[test]
    fn build_counts_all_records_and_fields() {
        let idx = index();
        assert_eq!(idx.len(), 4);
        assert!(!idx.is_empty());
        assert!(idx.is_indexed("lang"));
        assert!(idx.is_indexed("year"));
        assert!(!idx.is_indexed("missing"));
    }

    #[test]
    fn eq_on_indexed_field_resolves() {
        assert_eq!(
            cands(Filter::eq("lang", Value::String("rust".into()))),
            Some(vec![0, 2, 3])
        );
    }

    #[test]
    fn eq_with_no_postings_is_empty_not_none() {
        assert_eq!(
            cands(Filter::eq("lang", Value::String("zig".into()))),
            Some(vec![])
        );
    }

    #[test]
    fn eq_on_unindexed_field_is_none() {
        assert_eq!(
            cands(Filter::eq("author", Value::String("ada".into()))),
            None
        );
    }

    #[test]
    fn float_literal_is_none() {
        assert_eq!(cands(Filter::eq("year", Value::Float(2026.0))), None);
    }

    #[test]
    fn in_resolves_to_union() {
        assert_eq!(
            cands(Filter::is_in(
                "lang",
                vec![Value::String("go".into()), Value::String("rust".into())]
            )),
            Some(vec![0, 1, 2, 3])
        );
    }

    #[test]
    fn and_intersects_resolvable_children() {
        // lang == rust AND year == 2026 -> only row 0.
        assert_eq!(
            cands(Filter::and(vec![
                Filter::eq("lang", Value::String("rust".into())),
                Filter::eq("year", Value::Int(2026)),
            ])),
            Some(vec![0])
        );
    }

    #[test]
    fn and_narrows_using_only_the_resolvable_child() {
        // lang == rust AND (range, unresolved) -> narrowed to rust rows; the
        // range is left for `evaluate`.
        assert_eq!(
            cands(Filter::and(vec![
                Filter::eq("lang", Value::String("rust".into())),
                Filter::gt("year", Value::Int(2021)),
            ])),
            Some(vec![0, 2, 3])
        );
    }

    #[test]
    fn or_with_unresolvable_child_is_none() {
        assert_eq!(
            cands(Filter::or(vec![
                Filter::eq("lang", Value::String("rust".into())),
                Filter::gt("year", Value::Int(2021)),
            ])),
            None
        );
    }

    #[test]
    fn not_and_ranges_are_none() {
        assert_eq!(
            cands(Filter::not(Filter::eq(
                "lang",
                Value::String("rust".into())
            ))),
            None
        );
        assert_eq!(cands(Filter::gt("year", Value::Int(2000))), None);
    }

    #[test]
    fn candidates_are_a_superset_of_true_matches() {
        // For the narrowing-And case, every true match must be present.
        let rows = corpus();
        let filter = Filter::and(vec![
            Filter::eq("lang", Value::String("rust".into())),
            Filter::gt("year", Value::Int(2021)),
        ]);
        let evaluator = FilterEvaluator::new(filter).unwrap();
        let candidates: std::collections::HashSet<usize> = index()
            .candidates(&evaluator)
            .unwrap()
            .into_iter()
            .collect();

        for (k, m) in &rows {
            if evaluator.evaluate(Some(m)) {
                assert!(
                    candidates.contains(k),
                    "true match {k} missing from candidates"
                );
            }
        }
    }

    #[test]
    fn index_backed_selectivity_uses_real_counts() {
        let idx = index();
        // 3 of 4 rows are rust.
        let rust = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into()))).unwrap();
        assert!((idx.estimate_selectivity(&rust) - 0.75).abs() < 1e-9);
        // 1 of 4 rows has year == 2026.
        let y = FilterEvaluator::new(Filter::eq("year", Value::Int(2026))).unwrap();
        assert!((idx.estimate_selectivity(&y) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn empty_index_falls_back_to_structural() {
        let empty: MetadataIndex<usize> = MetadataIndex::build(&["lang"], std::iter::empty());
        assert!(empty.is_empty());
        let eq = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into()))).unwrap();
        // Structural EQ estimate, not a divide-by-zero.
        let s = empty.estimate_selectivity(&eq);
        assert!((0.0..=1.0).contains(&s));
    }

    #[test]
    fn records_without_metadata_count_but_do_not_post() {
        let rows: Vec<(usize, Option<&Metadata>)> = vec![(0, None), (1, None)];
        let idx: MetadataIndex<usize> = MetadataIndex::build(&["lang"], rows);
        assert_eq!(idx.len(), 2);
        let eq = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into()))).unwrap();
        assert_eq!(idx.candidates(&eq), Some(vec![]));
    }
}
