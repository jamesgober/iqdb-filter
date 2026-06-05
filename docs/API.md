# iqdb-filter &mdash; API Reference

> Complete reference for **every** public item in `iqdb-filter` as of
> **v0.2.0**: what it is, its parameters and return shape, and worked examples
> for each use case.
>
> **Status: pre-1.0.** The public API is being designed across the 0.x series
> and frozen at `1.0.0`. Items marked _(planned)_ describe surface that lands
> with the first approximate-index consumer; see `dev/ROADMAP.md`.

## Table of Contents

- [Overview](#overview)
- [Crate constants](#crate-constants)
  - [`VERSION`](#version)
  - [`MAX_FILTER_DEPTH`](#max_filter_depth)
  - [`MAX_IN_VALUES`](#max_in_values)
- [Evaluating a filter](#evaluating-a-filter)
  - [`FilterEvaluator`](#filterevaluator)
  - [`FilterEvaluator::new`](#filterevaluatornew)
  - [`FilterEvaluator::evaluate`](#filterevaluatorevaluate)
  - [`FilterEvaluator::filter`](#filterevaluatorfilter)
- [Evaluation semantics](#evaluation-semantics)
- [`FilterStrategy`](#filterstrategy)
- [Errors](#errors)
- [Feature flags](#feature-flags)
- [Trait implementation matrix](#trait-implementation-matrix)

---

## Overview

`iqdb-filter` is the metadata-filtering layer of the iQDB vector-database spine.
It owns one implementation of what an [`iqdb_types::Filter`] *means*, so every
index that supports filtering produces identical results.

The flow is two-phase, by design:

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};

// 1. Validate the filter ONCE — bounds depth and `In` width, returns a typed
//    error on violation.
let evaluator = FilterEvaluator::new(Filter::gt("year", Value::Int(2000)))
    .expect("valid filter");

// 2. Evaluate it per-row — infallible, allocation-free, never panics.
let meta: Metadata = [("year".to_string(), Value::Int(2026))].into_iter().collect();
assert!(evaluator.evaluate(Some(&meta)));
```

**Performance.** `evaluate` borrows the filter and the metadata and allocates
nothing; it is the path that runs once per candidate inside a search loop
(~19 ns for a representative compound predicate). Validation happens once in
`new` and is never repeated.

**No panics.** The sole fallible call is `new`, which returns
[`IqdbError::InvalidFilter`](#errors) for filters that breach the caps. Once
validated, `evaluate` cannot fail — including on records with no metadata, type
mismatches, and `NaN` values.

---

## Crate constants

### `VERSION`

```rust
pub const VERSION: &str;
```

The crate's compile-time version (`CARGO_PKG_VERSION`), a `major.minor.patch`
SemVer core. Use it to report the exact `iqdb-filter` build a binary links
against — useful in diagnostics and version-skew checks across the iQDB crate
family.

```rust
let v = iqdb_filter::VERSION;
assert_eq!(v.split('.').count(), 3);
assert!(v.split('.').all(|part| !part.is_empty()));
```

### `MAX_FILTER_DEPTH`

```rust
pub const MAX_FILTER_DEPTH: usize; // = 64
```

The maximum allowed nesting depth of a filter passed to
[`FilterEvaluator::new`](#filterevaluatornew). Each `And` / `Or` / `Not` node
adds one level; leaf comparisons do not. The cap sits well above any realistic
query and well below the recursion limit of every supported target's default
thread stack, so [`evaluate`](#filterevaluatorevaluate) cannot stack-overflow
on adversarial input. Exposed so higher layers (request parsers, query
builders) can quote the same number in their own validation.

```rust
assert!(iqdb_filter::MAX_FILTER_DEPTH >= 32);
```

### `MAX_IN_VALUES`

```rust
pub const MAX_IN_VALUES: usize; // = 1024
```

The maximum number of values in a single [`Filter::In`] node. `In` is
`O(|values|)` per candidate per query; the cap stops an attacker-supplied
predicate of a million values from turning every search into a denial of
service, while still covering realistic "tag in this set" queries.

```rust
assert!(iqdb_filter::MAX_IN_VALUES >= 256);
```

---

## Evaluating a filter

### `FilterEvaluator`

```rust
pub struct FilterEvaluator { /* private */ }
```

A validated [`iqdb_types::Filter`] paired with the canonical evaluator. Build
one with [`new`](#filterevaluatornew); the filter is walked once at construction
to enforce [`MAX_FILTER_DEPTH`](#max_filter_depth) and
[`MAX_IN_VALUES`](#max_in_values). After that,
[`evaluate`](#filterevaluatorevaluate) is infallible and may be called per row
without revalidation.

Derives `Debug` and `Clone`. Cloning copies the inner filter tree.

### `FilterEvaluator::new`

```rust
pub fn new(filter: Filter) -> iqdb_types::Result<FilterEvaluator>;
```

Validates `filter` and wraps it for evaluation.

- **`filter`** — the [`iqdb_types::Filter`] to validate and own.
- **Returns** — `Ok(FilterEvaluator)` on a well-formed filter, or
  `Err(`[`IqdbError::InvalidFilter`](#errors)`)` when the filter's nested-boolean
  depth exceeds [`MAX_FILTER_DEPTH`](#max_filter_depth) **or** any
  [`Filter::In`] node carries more than [`MAX_IN_VALUES`](#max_in_values)
  values.

The validation walk is **iterative** (an explicit work-list, not recursion), so
`new` itself cannot stack-overflow on a pathological input — even a filter
nested far past the cap returns a clean `Err`.

```rust
use iqdb_filter::{FilterEvaluator, MAX_IN_VALUES};
use iqdb_types::{Filter, IqdbError, Value};

// Accepted: a small, well-formed filter.
assert!(FilterEvaluator::new(Filter::eq("k", Value::Int(1))).is_ok());

// Rejected: an oversized `In`.
let huge = vec![Value::Int(0); MAX_IN_VALUES + 1];
let err = FilterEvaluator::new(Filter::is_in("tag", huge)).unwrap_err();
assert_eq!(err, IqdbError::InvalidFilter);
```

### `FilterEvaluator::evaluate`

```rust
pub fn evaluate(&self, metadata: Option<&Metadata>) -> bool;
```

Evaluates the validated filter against `metadata`.

- **`metadata`** — `Some(&Metadata)` for a record that carries metadata, or
  `None` for a record with none. Semantically `None` and an empty `Metadata`
  behave identically: every leaf evaluates to `false` (and `Not` of a leaf to
  `true`).
- **Returns** — `true` if the record matches the filter, `false` otherwise.

Infallible and allocation-free. This is the per-row hot path; the bounded depth
guaranteed by [`new`](#filterevaluatornew) keeps its recursive descent within a
fixed stack budget.

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};

let evaluator = FilterEvaluator::new(Filter::and(vec![
    Filter::eq("published", Value::Bool(true)),
    Filter::gt("year", Value::Int(2000)),
]))
.expect("valid filter");

let hit: Metadata = [
    ("published".to_string(), Value::Bool(true)),
    ("year".to_string(), Value::Int(2026)),
]
.into_iter()
.collect();
let miss: Metadata = [
    ("published".to_string(), Value::Bool(true)),
    ("year".to_string(), Value::Int(1999)),
]
.into_iter()
.collect();

assert!(evaluator.evaluate(Some(&hit)));
assert!(!evaluator.evaluate(Some(&miss)));
assert!(!evaluator.evaluate(None));
```

### `FilterEvaluator::filter`

```rust
pub fn filter(&self) -> &Filter;
```

Borrows the inner validated filter — useful for adapters that introspect the
predicate (logging, pushdown, statistics) without rebuilding it.

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Value};

let evaluator = FilterEvaluator::new(Filter::eq("k", Value::Int(1))).expect("valid");
assert!(matches!(evaluator.filter(), Filter::Eq { .. }));
```

---

## Evaluation semantics

The evaluator implements the **closed-world** rule pinned by
[`iqdb_types::Filter`]. These guarantees are covered by `tests/conformance.rs`
and `tests/properties.rs`:

| Situation | Result |
|-----------|--------|
| Leaf (`Eq`/`Neq`/`Lt`/`Lte`/`Gt`/`Gte`/`In`) over an **absent** field | `false` |
| Comparison between **mismatched types** (e.g. `Int` field vs `String` literal) | `false` |
| Ordered comparison involving `Value::Float(NaN)` | `false` (IEEE-754 unordered) |
| `Not` over a `false` leaf | `true` |
| Record with **no metadata** at all (`None`) | every leaf `false` |

A direct consequence: `Neq(absent)` is `false`, but `Not(Eq(absent))` is `true`
— the two are **not** interchangeable. The second is the idiom for "records that
do not have this field, or have it with a non-matching value":

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Value};

let neq = FilterEvaluator::new(Filter::neq("author", Value::String("ada".into())))
    .expect("valid");
let not_eq = FilterEvaluator::new(Filter::not(Filter::eq("author", Value::String("ada".into()))))
    .expect("valid");

// Same filter target, opposite answers on a record with no `author`.
assert!(!neq.evaluate(None));
assert!(not_eq.evaluate(None));
```

`And` requires every child to match; `Or` requires any child; both short-circuit.

---

## `FilterStrategy`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterStrategy {
    PreFilter,
    PostFilter,
    InFilter,
    Auto,
}
```

Vocabulary for how an index applies a metadata filter relative to its distance
scan. **v0.2 ships the variants only** — there is no selector yet; every
consumer applies pre-filtering through [`FilterEvaluator`](#filterevaluator) and
ignores this enum. It exists so `MetadataIndex`-driven indexes can adopt it
without a breaking change.

| Variant | Meaning |
|---------|---------|
| `PreFilter` | Apply the predicate **before** the distance computation; only matching candidates enter the scan. Cheap when selective. |
| `PostFilter` | Run the distance scan over every candidate, then drop hits that fail the predicate. Cheap when the predicate is broad. |
| `InFilter` | Interleave predicate evaluation with the distance walk so a graph index can prune branches. _(planned: requires `MetadataIndex` co-design.)_ |
| `Auto` | Let the index pick from the above based on estimated selectivity. _(planned: requires the selectivity machinery.)_ |

`#[non_exhaustive]`: do not match it exhaustively or assume the variant set is
closed.

```rust
use iqdb_filter::FilterStrategy;

let chosen = FilterStrategy::PreFilter;
assert_ne!(chosen, FilterStrategy::PostFilter);
```

---

## Errors

Every fallible call returns [`iqdb_types::Result`] (an alias for
`Result<T, iqdb_types::IqdbError>`). `iqdb-filter` produces exactly one variant:

| Variant | Raised by | When |
|---------|-----------|------|
| `IqdbError::InvalidFilter` | [`FilterEvaluator::new`](#filterevaluatornew) | the filter exceeds [`MAX_FILTER_DEPTH`](#max_filter_depth), or an `In` node exceeds [`MAX_IN_VALUES`](#max_in_values) |

`IqdbError` is `#[non_exhaustive]` and carries `ForgeError` metadata
(`kind()`, `caption()`) from `error-forge`. The variant carries no extra
context; callers that must distinguish "too deep" from "`In` too wide" can
re-walk the filter or pre-check against the public caps.

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| _(none)_ | — | The crate has no optional features. The default build is the canonical evaluator plus the `FilterStrategy` vocabulary, depending only on `iqdb-types`. |

---

## Trait implementation matrix

| Type | `Debug` | `Clone` | `Copy` | `PartialEq` / `Eq` | `Hash` |
|------|:-------:|:-------:|:------:|:------------------:|:------:|
| `FilterEvaluator` | ✓ | ✓ | | | |
| `FilterStrategy` | ✓ | ✓ | ✓ | ✓ | ✓ |

---

[`iqdb_types::Filter`]: https://docs.rs/iqdb-types/latest/iqdb_types/enum.Filter.html
[`Filter::In`]: https://docs.rs/iqdb-types/latest/iqdb_types/enum.Filter.html
[`iqdb_types::Result`]: https://docs.rs/iqdb-types/latest/iqdb_types/type.Result.html

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
