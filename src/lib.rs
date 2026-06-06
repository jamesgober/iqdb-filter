//! # iqdb-filter
//!
//! Canonical [`iqdb_types::Filter`] evaluator for the HiveDB **iqdb**
//! vector-database spine. One place that decides what `Filter` means; every
//! index that supports metadata filtering delegates to it.
//!
//! ## Why this lives outside the index crates
//!
//! Filtering used to be inlined in `iqdb-flat`. The moment a second index
//! (HNSW, IVF) starts honouring filters, two copies of the semantics would
//! drift — the `Neq(absent)` / `Not(Eq(absent))` rule is exactly the kind of
//! subtlety that splits between implementations and produces query-result
//! bugs nobody can attribute. Extracting the evaluator pins one set of
//! semantics across every consumer.
//!
//! ## Public surface
//!
//! - [`FilterEvaluator`] — `new(filter) -> Result<Self, IqdbError>` validates
//!   the filter once (depth, `In` cardinality); `evaluate(metadata) -> bool`
//!   is infallible on a validated filter. [`FilterEvaluator::prefilter`] and
//!   [`FilterEvaluator::postfilter`] apply it as lazy, allocation-free scan
//!   adapters over a stream of `(key, metadata)` pairs.
//! - [`estimate_selectivity`] — a best-effort, structural estimate of the
//!   fraction of records a validated filter passes, in `[0.0, 1.0]`.
//! - [`choose_strategy`] / [`StrategySelector`] — pick a concrete
//!   [`FilterStrategy`] from the selectivity estimate. The free function uses
//!   the [`DEFAULT_PREFILTER_THRESHOLD`]; the selector is the Tier-2 builder
//!   for tuning it.
//! - [`FilterStrategy`] — vocabulary for how an index applies a filter
//!   relative to its distance scan. The selector resolves `Auto` down to
//!   `PreFilter` / `PostFilter`; `InFilter` waits on a graph-index consumer.
//! - [`MAX_FILTER_DEPTH`] / [`MAX_IN_VALUES`] — documented validation caps,
//!   `pub const` so callers can quote them in error messages or higher-level
//!   validation.
//!
//! ## Null and absent-field semantics
//!
//! The evaluator implements the **closed-world** rule pinned by
//! [`iqdb_types::Filter`]: every leaf comparison (`Eq`, `Neq`, `Lt`, `Lte`,
//! `Gt`, `Gte`, `In`) over a field absent from the record's metadata
//! evaluates to `false`. Type mismatches between a stored value and a literal
//! also evaluate to `false`. `Value::Float(NaN)` under any ordered comparison
//! evaluates to `false` (IEEE-754 unordered). `Not` over a `false` leaf is
//! `true`, which is the idiom for "records without this field, or with a
//! non-matching value."
//!
//! `Neq(absent) → false` and `Not(Eq(absent)) → true` are therefore **not**
//! interchangeable. The pair is pinned by the conformance tests in
//! `tests/conformance.rs`.
//!
//! ## DoS hardening
//!
//! Construction is the validation gate. The walk is iterative (an explicit
//! stack, not recursion), so `new` cannot itself stack-overflow on
//! adversarial input. After construction every filter is bounded by
//! [`MAX_FILTER_DEPTH`], so the recursive [`FilterEvaluator::evaluate`] hot
//! path runs with a bounded call stack.
//!
//! ## Example
//!
//! ```
//! use iqdb_filter::FilterEvaluator;
//! use iqdb_types::{Filter, Metadata, Value};
//!
//! # fn main() -> iqdb_types::Result<()> {
//! let filter = Filter::and(vec![
//!     Filter::eq("published", Value::Bool(true)),
//!     Filter::gt("year", Value::Int(2000)),
//! ]);
//! let evaluator = FilterEvaluator::new(filter)?;
//!
//! let meta: Metadata = [
//!     ("published".to_string(), Value::Bool(true)),
//!     ("year".to_string(), Value::Int(2026)),
//! ]
//! .into_iter()
//! .collect();
//!
//! assert!(evaluator.evaluate(Some(&meta)));
//! assert!(!evaluator.evaluate(None));
//! # Ok(())
//! # }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![forbid(unsafe_code)]

mod eval;
mod evaluator;
mod selectivity;
mod strategy;

pub use crate::evaluator::{FilterEvaluator, MAX_FILTER_DEPTH, MAX_IN_VALUES};
pub use crate::selectivity::estimate_selectivity;
pub use crate::strategy::{
    DEFAULT_PREFILTER_THRESHOLD, FilterStrategy, StrategySelector, choose_strategy,
};

/// The version of this crate, taken from `Cargo.toml` at compile time.
///
/// Exposed so a consumer can report the exact `iqdb-filter` build it links
/// against — useful in diagnostics and version-skew checks across the iqdb
/// crate family.
///
/// # Examples
///
/// ```
/// // Carries a `major.minor.patch` SemVer core.
/// let version = iqdb_filter::VERSION;
/// assert_eq!(version.split('.').count(), 3);
/// assert!(version.split('.').all(|part| !part.is_empty()));
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
