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

#[cfg(test)]
mod tests {
    use super::*;

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
}
