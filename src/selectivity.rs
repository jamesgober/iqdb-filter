//! Structural selectivity estimation.
//!
//! [`estimate_selectivity`] guesses what fraction of records a filter will
//! pass, from the **shape** of the filter alone — it has no access to the
//! actual data distribution. That makes it cheap and always-available, but
//! deliberately coarse: it exists to rank a predicate as "narrow" or "broad"
//! so a strategy selector can choose between pre- and post-filtering, not to
//! predict an exact match count.
//!
//! Because the estimate is heuristic, the thresholds that act on it
//! ([`crate::StrategySelector`]) are tunable rather than baked in. When a real
//! per-field index with histograms or sketches lands, this function is the
//! seam an index-backed estimator replaces.

use iqdb_types::Filter;

use crate::FilterEvaluator;

/// Assumed fraction of records an equality leaf (`Eq`) matches. Equality on a
/// typical field selects a small slice of a corpus.
const EQ_SELECTIVITY: f64 = 0.1;

/// Assumed fraction an ordered-range leaf (`Lt` / `Lte` / `Gt` / `Gte`)
/// matches — a one-sided range is broader than equality but still a fraction.
const RANGE_SELECTIVITY: f64 = 1.0 / 3.0;

/// Estimate the fraction of records `evaluator`'s filter will pass, in
/// `[0.0, 1.0]` — `0.0` means "matches almost nothing", `1.0` means "matches
/// (almost) everything".
///
/// The estimate is **structural**: it is derived from the filter tree, not
/// from any data, using per-leaf base rates combined through the boolean
/// operators (`And` multiplies, `Or` unions, `Not` complements). It is a
/// best-effort hint a selector may use or ignore; do not treat it as an exact
/// probability.
///
/// Taking a validated [`FilterEvaluator`] (rather than a raw [`Filter`]) is
/// deliberate: the evaluator's depth is already bounded by
/// [`crate::MAX_FILTER_DEPTH`], so the recursive walk here cannot overflow on
/// adversarial input.
///
/// # Examples
///
/// ```
/// use iqdb_filter::{FilterEvaluator, estimate_selectivity};
/// use iqdb_types::{Filter, Value};
///
/// # fn main() -> iqdb_types::Result<()> {
/// // Equality is narrow; its negation is broad.
/// let eq = FilterEvaluator::new(Filter::eq("status", Value::Int(1)))?;
/// let neq = FilterEvaluator::new(Filter::neq("status", Value::Int(1)))?;
/// assert!(estimate_selectivity(&eq) < estimate_selectivity(&neq));
///
/// // `And` only narrows further.
/// let both = FilterEvaluator::new(Filter::and(vec![
///     Filter::eq("a", Value::Int(1)),
///     Filter::eq("b", Value::Int(2)),
/// ]))?;
/// assert!(estimate_selectivity(&both) <= estimate_selectivity(&eq));
///
/// // The estimate is always a probability.
/// assert!((0.0..=1.0).contains(&estimate_selectivity(&both)));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn estimate_selectivity(evaluator: &FilterEvaluator) -> f64 {
    estimate(evaluator.filter()).clamp(0.0, 1.0)
}

fn estimate(filter: &Filter) -> f64 {
    match filter {
        Filter::Eq { .. } => EQ_SELECTIVITY,
        Filter::Neq { .. } => 1.0 - EQ_SELECTIVITY,
        Filter::Lt { .. } | Filter::Lte { .. } | Filter::Gt { .. } | Filter::Gte { .. } => {
            RANGE_SELECTIVITY
        }
        // Each listed value is roughly one equality match; saturate at 1.0.
        // An empty `In` matches nothing (mirrors the evaluator's `any`).
        Filter::In { values, .. } => (EQ_SELECTIVITY * values.len() as f64).min(1.0),
        // Conjunction narrows: the product of independent fractions. An empty
        // `And` matches everything (mirrors the evaluator's `all`) -> 1.0.
        Filter::And(children) => children.iter().map(estimate).product(),
        // Disjunction widens: complement of "none match". An empty `Or` matches
        // nothing (mirrors the evaluator's `any`) -> 0.0.
        Filter::Or(children) => 1.0 - children.iter().map(|c| 1.0 - estimate(c)).product::<f64>(),
        Filter::Not(inner) => 1.0 - estimate(inner),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use iqdb_types::Value;

    fn sel(filter: Filter) -> f64 {
        estimate_selectivity(&FilterEvaluator::new(filter).unwrap())
    }

    #[test]
    fn eq_is_narrow_neq_is_broad() {
        assert!(sel(Filter::eq("k", Value::Int(1))) < 0.5);
        assert!(sel(Filter::neq("k", Value::Int(1))) > 0.5);
    }

    #[test]
    fn range_sits_between() {
        let eq = sel(Filter::eq("k", Value::Int(1)));
        let range = sel(Filter::gt("k", Value::Int(1)));
        let neq = sel(Filter::neq("k", Value::Int(1)));
        assert!(eq < range && range < neq);
    }

    #[test]
    fn and_narrows_or_widens() {
        let eq = sel(Filter::eq("a", Value::Int(1)));
        let and = sel(Filter::and(vec![
            Filter::eq("a", Value::Int(1)),
            Filter::eq("b", Value::Int(2)),
        ]));
        let or = sel(Filter::or(vec![
            Filter::eq("a", Value::Int(1)),
            Filter::eq("b", Value::Int(2)),
        ]));
        assert!(and < eq);
        assert!(or > eq);
    }

    #[test]
    fn not_complements() {
        let eq = sel(Filter::eq("k", Value::Int(1)));
        let not = sel(Filter::not(Filter::eq("k", Value::Int(1))));
        assert!((eq + not - 1.0).abs() < 1e-9);
    }

    #[test]
    fn in_scales_with_cardinality_and_saturates() {
        let one = sel(Filter::is_in("k", vec![Value::Int(1)]));
        let three = sel(Filter::is_in(
            "k",
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        ));
        assert!(three > one);
        // A wide `In` saturates at 1.0, never above.
        let wide = sel(Filter::is_in("k", vec![Value::Int(0); 100]));
        assert!((wide - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_and_matches_all_empty_or_matches_none() {
        assert!((sel(Filter::and(vec![])) - 1.0).abs() < 1e-9);
        assert!(sel(Filter::or(vec![])).abs() < 1e-9);
    }

    #[test]
    fn always_a_probability() {
        let deeply_nested = Filter::or(vec![
            Filter::not(Filter::and(vec![
                Filter::eq("a", Value::Int(1)),
                Filter::neq("b", Value::Int(2)),
            ])),
            Filter::is_in("c", vec![Value::Int(0); 50]),
        ]);
        let s = sel(deeply_nested);
        assert!((0.0..=1.0).contains(&s));
    }
}
