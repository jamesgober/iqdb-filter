//! [`FilterStrategy`] — vocabulary for how an index applies a [`Filter`].
//!
//! This release ships the vocabulary only. The four named strategies describe
//! the shapes a future selector will choose between; the selector itself, and
//! the `MetadataIndex` / cardinality machinery it needs, land in a later
//! release (see `dev/ROADMAP.md`). Today every consumer applies pre-filtering
//! through [`crate::FilterEvaluator`] and ignores this enum — the enum exists
//! so `MetadataIndex`-driven indexes can adopt it without a breaking change.
//!
//! [`Filter`]: iqdb_types::Filter

/// How an index plans to apply a metadata [`Filter`](iqdb_types::Filter)
/// relative to its distance scan.
///
/// `#[non_exhaustive]` because selection logic will add variants (and likely
/// adapter parameters) as approximate indexes start honouring filters.
/// Callers should not exhaustively match on this enum and should not rely on
/// the variant set being closed.
///
/// # Variants in plain terms
///
/// - [`FilterStrategy::PreFilter`] — apply the predicate **before** the
///   distance computation; only matching candidates enter the scan. Cheap
///   when the predicate is selective, wasteful when it is broad.
/// - [`FilterStrategy::PostFilter`] — run the distance scan over every
///   candidate, then drop hits that fail the predicate. Cheap when the
///   predicate is broad, defeats top-`k` truncation when the predicate is
///   selective (you may have to scan far past `k` to refill the result set).
/// - [`FilterStrategy::InFilter`] — interleave predicate evaluation with the
///   distance walk so a graph index can prune branches it knows can't
///   produce surviving candidates. Requires `MetadataIndex` co-design.
/// - [`FilterStrategy::Auto`] — let the index pick from the above based on
///   estimated selectivity. Requires the selectivity machinery that doesn't
///   exist yet; documented here so future configs can name it.
///
/// # Examples
///
/// ```
/// use iqdb_filter::FilterStrategy;
///
/// let chosen = FilterStrategy::PreFilter;
/// // Callers do not branch on this yet; the enum is vocabulary for later.
/// assert_ne!(chosen, FilterStrategy::PostFilter);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterStrategy {
    /// Apply the predicate before the distance scan.
    PreFilter,
    /// Apply the predicate after the distance scan.
    PostFilter,
    /// Interleave the predicate with a graph traversal.
    InFilter,
    /// Let the index pick the strategy based on estimated selectivity.
    Auto,
}

/// Default cutoff [`StrategySelector`] uses to split `PreFilter` from
/// `PostFilter`: a filter whose estimated selectivity is at or below this value
/// is treated as narrow enough to pre-filter.
///
/// `0.5` means "pre-filter when the predicate is expected to drop at least half
/// the corpus". It is a deliberately neutral default; tune it for a specific
/// index with [`StrategySelector::with_prefilter_threshold`].
pub const DEFAULT_PREFILTER_THRESHOLD: f64 = 0.5;

/// Picks a concrete [`FilterStrategy`] for a validated filter from its
/// estimated selectivity — the Tier-2, tunable counterpart to
/// [`choose_strategy`].
///
/// The rule is simple and monotone: a **narrow** predicate (low
/// [`estimate_selectivity`](crate::estimate_selectivity), at or below the
/// threshold) resolves to [`FilterStrategy::PreFilter`], because evaluating it
/// up front skips the distance computation for the rows it rejects; a **broad**
/// predicate resolves to [`FilterStrategy::PostFilter`], because pre-filtering
/// would materialise nearly the whole corpus for little gain. The selector
/// never returns [`FilterStrategy::Auto`] (it is the thing that resolves it) or
/// [`FilterStrategy::InFilter`] (which needs graph-traversal co-design).
///
/// The type is immutable: [`with_prefilter_threshold`](Self::with_prefilter_threshold)
/// returns a new selector rather than mutating in place.
///
/// # Examples
///
/// ```
/// use iqdb_filter::{FilterEvaluator, FilterStrategy, StrategySelector};
/// use iqdb_types::{Filter, Value};
///
/// # fn main() -> iqdb_types::Result<()> {
/// let selector = StrategySelector::new().with_prefilter_threshold(0.3);
///
/// let narrow = FilterEvaluator::new(Filter::eq("id", Value::Int(7)))?;
/// let broad = FilterEvaluator::new(Filter::neq("id", Value::Int(7)))?;
///
/// assert_eq!(selector.choose(&narrow), FilterStrategy::PreFilter);
/// assert_eq!(selector.choose(&broad), FilterStrategy::PostFilter);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct StrategySelector {
    prefilter_threshold: f64,
}

impl Default for StrategySelector {
    fn default() -> Self {
        Self {
            prefilter_threshold: DEFAULT_PREFILTER_THRESHOLD,
        }
    }
}

impl StrategySelector {
    /// Creates a selector with the [`DEFAULT_PREFILTER_THRESHOLD`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::{StrategySelector, DEFAULT_PREFILTER_THRESHOLD};
    ///
    /// let selector = StrategySelector::new();
    /// assert_eq!(selector.prefilter_threshold(), DEFAULT_PREFILTER_THRESHOLD);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new selector that pre-filters when estimated selectivity is at
    /// or below `threshold`.
    ///
    /// `threshold` is clamped to `[0.0, 1.0]`: `0.0` pre-filters only the most
    /// extreme predicates (effectively always post-filter), `1.0` always
    /// pre-filters.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::StrategySelector;
    ///
    /// let always_pre = StrategySelector::new().with_prefilter_threshold(1.0);
    /// assert_eq!(always_pre.prefilter_threshold(), 1.0);
    ///
    /// // Out-of-range values are clamped, never panic.
    /// let clamped = StrategySelector::new().with_prefilter_threshold(2.5);
    /// assert_eq!(clamped.prefilter_threshold(), 1.0);
    /// ```
    #[must_use]
    pub fn with_prefilter_threshold(self, threshold: f64) -> Self {
        Self {
            prefilter_threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// The selectivity cutoff this selector uses.
    #[must_use]
    pub fn prefilter_threshold(&self) -> f64 {
        self.prefilter_threshold
    }

    /// Resolves the strategy for `evaluator`'s filter.
    ///
    /// Returns [`FilterStrategy::PreFilter`] when the estimated selectivity is
    /// at or below [`prefilter_threshold`](Self::prefilter_threshold), and
    /// [`FilterStrategy::PostFilter`] otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb_filter::{FilterEvaluator, FilterStrategy, StrategySelector};
    /// use iqdb_types::{Filter, Value};
    ///
    /// # fn main() -> iqdb_types::Result<()> {
    /// let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1)))?;
    /// assert_eq!(StrategySelector::new().choose(&evaluator), FilterStrategy::PreFilter);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn choose(&self, evaluator: &crate::FilterEvaluator) -> FilterStrategy {
        if crate::estimate_selectivity(evaluator) <= self.prefilter_threshold {
            FilterStrategy::PreFilter
        } else {
            FilterStrategy::PostFilter
        }
    }
}

/// Picks a concrete [`FilterStrategy`] for a validated filter using the
/// [`DEFAULT_PREFILTER_THRESHOLD`] — the Tier-1 shortcut for
/// [`StrategySelector::new().choose(..)`](StrategySelector::choose).
///
/// Returns [`FilterStrategy::PreFilter`] for narrow predicates and
/// [`FilterStrategy::PostFilter`] for broad ones; never `Auto` or `InFilter`.
///
/// # Examples
///
/// ```
/// use iqdb_filter::{FilterEvaluator, FilterStrategy, choose_strategy};
/// use iqdb_types::{Filter, Value};
///
/// # fn main() -> iqdb_types::Result<()> {
/// let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1)))?;
/// assert_eq!(choose_strategy(&evaluator), FilterStrategy::PreFilter);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn choose_strategy(evaluator: &crate::FilterEvaluator) -> FilterStrategy {
    StrategySelector::new().choose(evaluator)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::FilterEvaluator;
    use iqdb_types::{Filter, Value};

    #[test]
    fn variants_compare_by_value() {
        assert_eq!(FilterStrategy::PreFilter, FilterStrategy::PreFilter);
        assert_ne!(FilterStrategy::PreFilter, FilterStrategy::Auto);
    }

    #[test]
    fn variants_are_copy() {
        let a = FilterStrategy::InFilter;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn default_threshold_is_the_constant() {
        assert_eq!(
            StrategySelector::new().prefilter_threshold(),
            DEFAULT_PREFILTER_THRESHOLD
        );
    }

    #[test]
    fn threshold_is_clamped() {
        assert_eq!(
            StrategySelector::new()
                .with_prefilter_threshold(-1.0)
                .prefilter_threshold(),
            0.0
        );
        assert_eq!(
            StrategySelector::new()
                .with_prefilter_threshold(9.0)
                .prefilter_threshold(),
            1.0
        );
    }

    #[test]
    fn narrow_prefilters_broad_postfilters() {
        let narrow = FilterEvaluator::new(Filter::eq("k", Value::Int(1))).unwrap();
        let broad = FilterEvaluator::new(Filter::neq("k", Value::Int(1))).unwrap();
        assert_eq!(choose_strategy(&narrow), FilterStrategy::PreFilter);
        assert_eq!(choose_strategy(&broad), FilterStrategy::PostFilter);
    }

    #[test]
    fn threshold_extremes_force_one_strategy() {
        let broad = FilterEvaluator::new(Filter::neq("k", Value::Int(1))).unwrap();
        let always_pre = StrategySelector::new().with_prefilter_threshold(1.0);
        let always_post = StrategySelector::new().with_prefilter_threshold(0.0);
        assert_eq!(always_pre.choose(&broad), FilterStrategy::PreFilter);
        assert_eq!(always_post.choose(&broad), FilterStrategy::PostFilter);
    }
}
