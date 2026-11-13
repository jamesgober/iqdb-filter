//! [`FilterEvaluator`] — the public, validated face of the evaluator.
//!
//! Construction is the validation gate. After [`FilterEvaluator::new`]
//! returns `Ok`, the filter is known to satisfy the depth, `In`-cardinality
//! and total-size caps documented on [`MAX_FILTER_DEPTH`], [`MAX_IN_VALUES`],
//! [`MAX_FILTER_NODES`] and [`MAX_FILTER_VALUES`], and the recursive
//! [`FilterEvaluator::evaluate`] hot path can run without checking any bound
//! again.

use iqdb_types::{Filter, IqdbError, Metadata, Result};

use crate::eval;

/// Maximum allowed nesting depth of a [`Filter`] passed to
/// [`FilterEvaluator::new`].
///
/// Each `And`/`Or`/`Not` node adds one level of nesting; leaf comparisons
/// (`Eq`, `Neq`, `Lt`, `Lte`, `Gt`, `Gte`, `In`) do not. A depth of `64`
/// already exceeds anything a hand-written or generated query is expected to
/// produce, while sitting well below the recursion limit of every supported
/// target's default thread stack. Filters that exceed the cap are rejected
/// with [`IqdbError::InvalidFilter`] so [`FilterEvaluator::evaluate`] cannot
/// stack-overflow on adversarial input.
///
/// Exposed as `pub const` so higher-level validation layers (request parsers,
/// query builders) can quote the same number in their own error messages.
///
/// # Examples
///
/// ```
/// assert!(iqdb_filter::MAX_FILTER_DEPTH >= 32);
/// ```
pub const MAX_FILTER_DEPTH: usize = 64;

/// Maximum allowed number of values in a single [`Filter::In`] node.
///
/// `Filter::In` is `O(|values|)` per candidate per query; without a cap, an
/// attacker-supplied predicate of a million values turns every search into a
/// denial-of-service. `1024` covers realistic "tag in this set" queries while
/// keeping per-row evaluation cheap. Filters exceeding the cap are rejected
/// with [`IqdbError::InvalidFilter`].
///
/// Exposed as `pub const` so higher layers can pre-check before reaching the
/// evaluator.
///
/// # Examples
///
/// ```
/// assert!(iqdb_filter::MAX_IN_VALUES >= 256);
/// ```
pub const MAX_IN_VALUES: usize = 1024;

/// Maximum total number of nodes (leaves plus `And`/`Or`/`Not`) in a
/// [`Filter`] passed to [`FilterEvaluator::new`].
///
/// The depth and per-`In` caps alone do not bound the cost of a filter: an
/// `And` may hold any number of children, each evaluated for every record.
/// This cap bounds the per-record work together with
/// [`MAX_FILTER_VALUES`]. `4096` is far beyond any realistic query. Filters
/// that exceed it are rejected with [`IqdbError::InvalidFilter`].
///
/// # Examples
///
/// ```
/// assert!(iqdb_filter::MAX_FILTER_NODES >= 1024);
/// ```
pub const MAX_FILTER_NODES: usize = 4096;

/// Maximum total number of values across every [`Filter::In`] node of one
/// [`Filter`] passed to [`FilterEvaluator::new`].
///
/// Each `In` is already capped at [`MAX_IN_VALUES`]; this caps their sum,
/// so many wide `In` nodes cannot multiply the per-record cost. Filters
/// that exceed it are rejected with [`IqdbError::InvalidFilter`].
///
/// # Examples
///
/// ```
/// assert!(iqdb_filter::MAX_FILTER_VALUES >= iqdb_filter::MAX_IN_VALUES);
/// ```
pub const MAX_FILTER_VALUES: usize = 16_384;

/// A validated [`Filter`] paired with the canonical evaluator.
///
/// Build one with [`FilterEvaluator::new`]; the filter is walked once at
/// construction to enforce [`MAX_FILTER_DEPTH`], [`MAX_IN_VALUES`],
/// [`MAX_FILTER_NODES`] and [`MAX_FILTER_VALUES`]. After
/// that, [`FilterEvaluator::evaluate`] is infallible and may be called per
/// row inside a search loop without revalidation.
///
/// # Examples
///
/// ```
/// use iqdb_filter::FilterEvaluator;
/// use iqdb_types::{Filter, Metadata, Value};
///
/// # fn main() -> iqdb_types::Result<()> {
/// let evaluator = FilterEvaluator::new(Filter::eq("year", Value::Int(2026)))?;
///
/// let meta: Metadata =
///     [("year".to_string(), Value::Int(2026))].into_iter().collect();
///
/// assert!(evaluator.evaluate(Some(&meta)));
/// assert!(!evaluator.evaluate(None));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct FilterEvaluator {
    filter: Filter,
}

impl FilterEvaluator {
    /// Validates `filter` and wraps it for evaluation.
    ///
    /// The validation walk is iterative — an explicit work-list, not
    /// recursion — so `new` itself cannot stack-overflow on a pathological
    /// input. It returns [`IqdbError::InvalidFilter`] when:
    ///
    /// - the filter's nested-boolean depth exceeds [`MAX_FILTER_DEPTH`];
    /// - any [`Filter::In`] node carries more than [`MAX_IN_VALUES`] values;
    /// - the filter has more than [`MAX_FILTER_NODES`] nodes in total; or
    /// - its `In` nodes carry more than [`MAX_FILTER_VALUES`] values in total.
    ///
    /// The walk stops at the first violation, so rejecting an oversized
    /// filter costs no more than the caps themselves. To check a filter you
    /// only hold by reference without cloning it first, use
    /// [`FilterEvaluator::try_from`] on a `&Filter`.
    ///
    /// # Errors
    ///
    /// Returns [`IqdbError::InvalidFilter`] for filters that violate any
    /// cap. The variant carries no extra context — callers that need to
    /// distinguish "too deep" from "`In` too wide" can re-walk the filter
    /// themselves or pre-validate against the public consts.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, IqdbError, Value};
    ///
    /// // Accepted: a small, well-formed filter.
    /// let ok = FilterEvaluator::new(Filter::eq("k", Value::Int(1)));
    /// assert!(ok.is_ok());
    ///
    /// // Rejected: an oversized `In`.
    /// let huge = vec![Value::Int(0); iqdb_filter::MAX_IN_VALUES + 1];
    /// let err = FilterEvaluator::new(Filter::is_in("tag", huge)).unwrap_err();
    /// assert_eq!(err, IqdbError::InvalidFilter);
    /// ```
    pub fn new(filter: Filter) -> Result<Self> {
        validate(&filter)?;
        Ok(Self { filter })
    }
}

impl TryFrom<&Filter> for FilterEvaluator {
    type Error = IqdbError;

    /// Validates a borrowed `filter` and clones it only once it passes, so
    /// an oversized filter is rejected without copying it.
    ///
    /// Same caps and errors as [`FilterEvaluator::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, Value};
    ///
    /// let filter = Filter::eq("k", Value::Int(1));
    /// let evaluator = FilterEvaluator::try_from(&filter).unwrap();
    /// assert_eq!(evaluator.filter(), &filter);
    /// ```
    fn try_from(filter: &Filter) -> Result<Self> {
        validate(filter)?;
        Ok(Self {
            filter: filter.clone(),
        })
    }
}

impl FilterEvaluator {
    /// Evaluates the validated filter against `metadata`.
    ///
    /// `None` means the record has no metadata at all — distinct from an
    /// empty `Metadata` only at the API boundary; semantically every leaf
    /// over `None` and every leaf over an empty `Metadata` evaluates to
    /// `false` (and `Not` over either evaluates to `true`).
    ///
    /// This call is infallible: validation happened in
    /// [`FilterEvaluator::new`], so the recursive descent is bounded by
    /// [`MAX_FILTER_DEPTH`] and cannot stack-overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, Metadata, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let evaluator =
    ///     FilterEvaluator::new(Filter::not(Filter::eq("author", Value::String("ada".into()))))?;
    ///
    /// // No metadata → leaf is false → Not flips it to true. This is the
    /// // documented "records without this field" idiom.
    /// assert!(evaluator.evaluate(None));
    ///
    /// let meta: Metadata = [(
    ///     "author".to_string(),
    ///     Value::String("ada".into()),
    /// )]
    /// .into_iter()
    /// .collect();
    /// assert!(!evaluator.evaluate(Some(&meta)));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn evaluate(&self, metadata: Option<&Metadata>) -> bool {
        eval::eval(&self.filter, metadata)
    }

    /// Pre-filter a stream of candidates: yield the key of each candidate whose
    /// metadata matches, **before** any distance is computed.
    ///
    /// This is the [`FilterStrategy::PreFilter`](crate::FilterStrategy::PreFilter)
    /// shape — reduce the candidate set first, then score only the survivors.
    /// It is the pattern an exact index uses to skip the distance computation
    /// for rows the predicate already rejects.
    ///
    /// The adapter is lazy and allocation-free: it borrows each candidate's
    /// metadata and forwards the key untouched. `key` is whatever a caller uses
    /// to identify a row — a storage index, a [`iqdb_types::VectorId`], a tuple.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, Metadata, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let evaluator = FilterEvaluator::new(Filter::gt("year", Value::Int(2000)))?;
    ///
    /// let m2026: Metadata = [("year".to_string(), Value::Int(2026))].into_iter().collect();
    /// let m1999: Metadata = [("year".to_string(), Value::Int(1999))].into_iter().collect();
    /// let rows = [(0_usize, Some(&m2026)), (1, Some(&m1999)), (2, None)];
    ///
    /// let kept: Vec<usize> = evaluator.prefilter(rows).collect();
    /// assert_eq!(kept, [0]); // only the 2026 row survives
    /// # Ok(())
    /// # }
    /// ```
    pub fn prefilter<'a, K, I>(&'a self, candidates: I) -> impl Iterator<Item = K> + 'a
    where
        I: IntoIterator<Item = (K, Option<&'a Metadata>)>,
        I::IntoIter: 'a,
        K: 'a,
    {
        candidates
            .into_iter()
            .filter_map(move |(key, metadata)| self.evaluate(metadata).then_some(key))
    }

    /// Post-filter a stream of already-scored results: yield each hit whose
    /// metadata matches, **after** the distance scan has ranked candidates.
    ///
    /// This is the [`FilterStrategy::PostFilter`](crate::FilterStrategy::PostFilter)
    /// shape — score everything, then drop the hits the predicate rejects. It
    /// shares the per-row test with [`prefilter`](Self::prefilter); the
    /// difference is purely where in the pipeline it runs. Because it is lazy,
    /// a caller refilling a top-`k` result set can chain `.take(k)` and stop as
    /// soon as `k` survivors are found.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, Metadata, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let evaluator = FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into())))?;
    ///
    /// let rust: Metadata = [("lang".to_string(), Value::String("rust".into()))]
    ///     .into_iter()
    ///     .collect();
    /// let go: Metadata = [("lang".to_string(), Value::String("go".into()))]
    ///     .into_iter()
    ///     .collect();
    ///
    /// // Hits arrive sorted by distance; keep the first matching one.
    /// let scored = [("hit-a", Some(&go)), ("hit-b", Some(&rust))];
    /// let best: Vec<&str> = evaluator.postfilter(scored).take(1).collect();
    /// assert_eq!(best, ["hit-b"]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`prefilter`]: Self::prefilter
    pub fn postfilter<'a, H, I>(&'a self, scored: I) -> impl Iterator<Item = H> + 'a
    where
        I: IntoIterator<Item = (H, Option<&'a Metadata>)>,
        I::IntoIter: 'a,
        H: 'a,
    {
        scored
            .into_iter()
            .filter_map(move |(hit, metadata)| self.evaluate(metadata).then_some(hit))
    }

    /// Borrows the inner validated filter.
    ///
    /// Useful for adapters that want to introspect the predicate (for
    /// logging, pushdown, or statistics) without rebuilding it.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::FilterEvaluator;
    /// use iqdb_types::{Filter, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1)))?;
    /// assert!(matches!(evaluator.filter(), Filter::Eq { .. }));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn filter(&self) -> &Filter {
        &self.filter
    }
}

// One work-list entry: a borrowed sub-filter plus the depth of its parent.
// The node itself counts toward depth only if it is an And/Or/Not (handled
// inside `validate`).
struct Frame<'a> {
    node: &'a Filter,
    parent_depth: usize,
}

fn validate(root: &Filter) -> Result<()> {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    stack.push(Frame {
        node: root,
        parent_depth: 0,
    });

    // Nodes pushed onto the work list so far, the root included. Checked
    // before each push, so neither the walk nor the list can outgrow the
    // node cap.
    let mut queued: usize = 1;
    let mut in_values: usize = 0;
    while let Some(Frame { node, parent_depth }) = stack.pop() {
        match node {
            Filter::In { values, .. } => {
                if values.len() > MAX_IN_VALUES {
                    return Err(IqdbError::InvalidFilter);
                }
                in_values = in_values.saturating_add(values.len());
                if in_values > MAX_FILTER_VALUES {
                    return Err(IqdbError::InvalidFilter);
                }
            }
            Filter::Eq { .. }
            | Filter::Neq { .. }
            | Filter::Lt { .. }
            | Filter::Lte { .. }
            | Filter::Gt { .. }
            | Filter::Gte { .. } => {}
            Filter::And(children) | Filter::Or(children) => {
                let depth = parent_depth.saturating_add(1);
                if depth > MAX_FILTER_DEPTH {
                    return Err(IqdbError::InvalidFilter);
                }
                if children.len() > MAX_FILTER_NODES - queued {
                    return Err(IqdbError::InvalidFilter);
                }
                queued += children.len();
                for child in children {
                    stack.push(Frame {
                        node: child,
                        parent_depth: depth,
                    });
                }
            }
            Filter::Not(inner) => {
                let depth = parent_depth.saturating_add(1);
                if depth > MAX_FILTER_DEPTH || queued >= MAX_FILTER_NODES {
                    return Err(IqdbError::InvalidFilter);
                }
                queued += 1;
                stack.push(Frame {
                    node: inner,
                    parent_depth: depth,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use iqdb_types::Value;

    fn nested_not(depth: usize) -> Filter {
        let mut current = Filter::eq("k", Value::Int(0));
        for _ in 0..depth {
            current = Filter::not(current);
        }
        current
    }

    #[test]
    fn new_accepts_a_leaf() {
        let f = Filter::eq("k", Value::Int(1));
        assert!(FilterEvaluator::new(f).is_ok());
    }

    #[test]
    fn new_accepts_at_max_depth() {
        // `MAX_FILTER_DEPTH` Not nodes wrapping a leaf — depth equals the cap.
        let f = nested_not(MAX_FILTER_DEPTH);
        assert!(FilterEvaluator::new(f).is_ok());
    }

    #[test]
    fn new_rejects_just_over_max_depth() {
        let f = nested_not(MAX_FILTER_DEPTH + 1);
        let err = FilterEvaluator::new(f).unwrap_err();
        assert_eq!(err, IqdbError::InvalidFilter);
    }

    #[test]
    fn new_accepts_in_at_cap() {
        let f = Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES]);
        assert!(FilterEvaluator::new(f).is_ok());
    }

    #[test]
    fn new_rejects_in_just_over_cap() {
        let f = Filter::is_in("tag", vec![Value::Int(0); MAX_IN_VALUES + 1]);
        let err = FilterEvaluator::new(f).unwrap_err();
        assert_eq!(err, IqdbError::InvalidFilter);
    }

    #[test]
    fn evaluate_matches_validated_filter() {
        let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1))).unwrap();
        let meta: Metadata = [("k".to_string(), Value::Int(1))].into_iter().collect();
        assert!(evaluator.evaluate(Some(&meta)));
        assert!(!evaluator.evaluate(None));
    }

    #[test]
    fn evaluator_clone_preserves_filter() {
        let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1))).unwrap();
        let copy = evaluator.clone();
        let meta: Metadata = [("k".to_string(), Value::Int(1))].into_iter().collect();
        assert_eq!(evaluator.evaluate(Some(&meta)), copy.evaluate(Some(&meta)));
    }

    fn meta(field: &str, value: Value) -> Metadata {
        [(field.to_string(), value)].into_iter().collect()
    }

    #[test]
    fn prefilter_keeps_only_matching_keys() {
        let evaluator = FilterEvaluator::new(Filter::gt("year", Value::Int(2000))).unwrap();
        let m2026 = meta("year", Value::Int(2026));
        let m1999 = meta("year", Value::Int(1999));
        let rows = [(10_usize, Some(&m2026)), (20, Some(&m1999)), (30, None)];

        let kept: Vec<usize> = evaluator.prefilter(rows).collect();
        assert_eq!(kept, [10]);
    }

    #[test]
    fn postfilter_keeps_only_matching_hits_and_is_lazy() {
        let evaluator =
            FilterEvaluator::new(Filter::eq("lang", Value::String("rust".into()))).unwrap();
        let rust = meta("lang", Value::String("rust".into()));
        let go = meta("lang", Value::String("go".into()));
        let scored = [("a", Some(&go)), ("b", Some(&rust)), ("c", Some(&rust))];

        let first: Vec<&str> = evaluator.postfilter(scored).take(1).collect();
        assert_eq!(first, ["b"]);
    }

    #[test]
    fn prefilter_empty_input_yields_nothing() {
        let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1))).unwrap();
        let rows: [(usize, Option<&Metadata>); 0] = [];
        assert_eq!(evaluator.prefilter(rows).count(), 0);
    }
}
