<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>iqdb-filter</b>
    <br>
    <sub><sup>iQDB HYBRID FILTERING</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/iqdb-filter"><img alt="Crates.io" src="https://img.shields.io/crates/v/iqdb-filter"></a>
    <a href="https://crates.io/crates/iqdb-filter"><img alt="Downloads" src="https://img.shields.io/crates/d/iqdb-filter?color=%230099ff"></a>
    <a href="https://docs.rs/iqdb-filter"><img alt="docs.rs" src="https://img.shields.io/docsrs/iqdb-filter"></a>
    <a href="https://github.com/jamesgober/iqdb-filter/actions"><img alt="CI" src="https://github.com/jamesgober/iqdb-filter/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.87%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>iqdb-filter</strong> is the metadata-filtering layer for the iQDB vector-database spine. It is the one place that decides what a <code>Filter</code> means: every index that honours metadata filters delegates here, so the semantics can never drift between implementations.
    </p>
    <p>
        It evaluates the <code>Filter</code> expression language defined in <a href="https://crates.io/crates/iqdb-types"><code>iqdb-types</code></a> against a record's metadata, with strict, predictable closed-world rules and validation that bounds every filter before it runs.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.87+</strong> (Rust 2024 edition). Validate once, evaluate per-row. No panics on hostile input. ~19 ns to evaluate a compound predicate.
    </p>
    <blockquote>
        <strong>Status: pre-1.0, in active development.</strong> The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

<h2>What it does</h2>

- **Canonical evaluator** &mdash; one implementation of `Filter` semantics, shared by every metadata-aware index so query results never diverge
- **Validate once, evaluate many** &mdash; `FilterEvaluator::new` checks the filter (depth, `In` cardinality) a single time; `evaluate` is then infallible and runs per-row inside the search loop
- **Closed-world semantics** &mdash; a leaf over an absent field is `false`, type mismatches are `false`, `NaN` orderings are `false`, and `Not` of a `false` leaf is `true` (the "records without this field" idiom)
- **DoS-hardened** &mdash; iterative validation that can't stack-overflow, with bounded depth and `In` width; the library never panics on adversarial input
- **Strategy vocabulary** &mdash; the `FilterStrategy` enum names the pre-/post-/in-traversal shapes a future selector will choose between
- **First-party only** &mdash; depends solely on `iqdb-types`, so it is unblocked today

<br>

## Installation

```toml
[dependencies]
iqdb-filter = "0.2"
```

<br>

## Quick start

Build an evaluator once, then test it against each record's metadata:

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Metadata, Value};

// published == true AND year > 2000
let filter = Filter::and(vec![
    Filter::eq("published", Value::Bool(true)),
    Filter::gt("year", Value::Int(2000)),
]);
let evaluator = FilterEvaluator::new(filter).expect("valid filter");

let meta: Metadata = [
    ("published".to_string(), Value::Bool(true)),
    ("year".to_string(), Value::Int(2026)),
]
.into_iter()
.collect();

assert!(evaluator.evaluate(Some(&meta)));
assert!(!evaluator.evaluate(None)); // no metadata -> every leaf is false
```

The `Not` / absent-field idiom selects records that *lack* a field, or carry it with a non-matching value:

```rust
use iqdb_filter::FilterEvaluator;
use iqdb_types::{Filter, Value};

// "records that are not authored by ada" — including records with no author.
let evaluator =
    FilterEvaluator::new(Filter::not(Filter::eq("author", Value::String("ada".into()))))
        .expect("valid filter");

assert!(evaluator.evaluate(None));
```

Validation rejects pathological filters up front — bounded by the public caps:

```rust
use iqdb_filter::{FilterEvaluator, MAX_IN_VALUES};
use iqdb_types::{Filter, IqdbError, Value};

// An `In` set wider than the cap is refused before it can slow a query.
let huge = vec![Value::Int(0); MAX_IN_VALUES + 1];
let err = FilterEvaluator::new(Filter::is_in("tag", huge)).unwrap_err();
assert_eq!(err, IqdbError::InvalidFilter);
```

<br>

## Errors

`FilterEvaluator::new` returns `iqdb_types::Result`; the only failure is
`IqdbError::InvalidFilter`, returned when a filter exceeds `MAX_FILTER_DEPTH`
nesting or carries an `In` node wider than `MAX_IN_VALUES`. After a filter is
validated, `evaluate` is infallible and never panics — including on records
with no metadata, type mismatches, and `NaN` values.

<br>

## Status

<code>v0.2.0</code> &mdash; the canonical evaluator has landed: `FilterEvaluator`
(validate-on-construction plus an infallible, allocation-free per-row
`evaluate`), the `FilterStrategy` vocabulary, and the public validation caps.
The closed-world semantics are pinned by integration and property tests, and the
surface is verified across the CI matrix (Linux, macOS, Windows) on stable and
the 1.87 MSRV. Selectivity estimation, automatic strategy selection, and the
optional inverted `MetadataIndex` are deferred until the first approximate-index
consumer lands — see the <a href="./dev/ROADMAP.md"><code>ROADMAP</code></a> for
the rationale. The full surface is documented in <a href="./docs/API.md"><code>docs/API.md</code></a>.

<hr>
<br>

## Where It Fits

`iqdb-filter` sits just above the types crate and is consumed by the index layer:

- `iqdb-types` &mdash; the `Filter`, `Metadata`, and `Value` types it evaluates
- `iqdb-flat` / `iqdb-hnsw` / `iqdb-ivf` &mdash; delegate here for metadata filtering
- `iqdb` &mdash; exposes filtered search to users

Its only first-party dependency is `iqdb-types`, so it is unblocked today.

<br>

## Standards

Built to the iQDB Rust standard. See <a href="./REPS.md"><code>REPS.md</code></a> (Rust Efficiency &amp; Performance Standards) and <a href="./dev/DIRECTIVES.md"><code>dev/DIRECTIVES.md</code></a> for the engineering law and the definition of done. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sup>
</div>
