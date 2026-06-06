# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [0.4.0] - 2026-06-05

Inverted metadata index. A selective `Eq` / `In` predicate can now resolve to a
candidate key set instead of scanning every row, and the strategy selector
gains a data-backed selectivity estimate.

### Added

- `MetadataIndex<K>` — an opt-in, per-field inverted index. `build(fields,
  records)` indexes only the named fields; `candidates(&FilterEvaluator)`
  returns a candidate key set (a superset of true matches; the caller confirms
  with `evaluate`) for resolvable `Eq` / `In` / `And` / `Or` predicates over
  `String` / `Int` / `Bool` / `Null` values, or `None` when it cannot bound the
  query (ranges, `Neq`, `Not`, `Float` literals, non-indexed fields). Includes
  `len`, `is_empty`, `is_indexed`, and `indexed_fields`.
- `MetadataIndex::estimate_selectivity` — a data-backed selectivity estimate
  using real posting counts where available, falling back to the structural
  estimate elsewhere.
- `StrategySelector::choose_with_index` — resolves a strategy from the
  index-backed estimate.

### Changed

- `FilterStrategy::Auto` resolution can now be index-informed via
  `choose_with_index`; the structural `choose` / `choose_strategy` are unchanged.

---

## [0.3.0] - 2026-06-05

Strategy selection. The `FilterStrategy` vocabulary becomes actionable: a
selectivity estimate drives an automatic `PreFilter` / `PostFilter` choice, and
the evaluator gains the scan helpers that apply each strategy.

### Added

- `FilterEvaluator::prefilter` &mdash; a lazy, allocation-free iterator adapter
  that keeps the keys of candidates whose metadata matches, before scoring
  (the `PreFilter` shape).
- `FilterEvaluator::postfilter` &mdash; the same per-row test applied to
  already-scored hits, lazy so a caller can `.take(k)` while refilling a
  top-`k` result set (the `PostFilter` shape).
- `estimate_selectivity` &mdash; a best-effort, structural estimate (in
  `[0.0, 1.0]`) of the fraction of records a validated filter passes. Takes a
  `FilterEvaluator`, so the walk is depth-bounded and cannot overflow.
- `choose_strategy` (Tier 1) and `StrategySelector` (Tier 2, tunable
  `prefilter_threshold`) &mdash; resolve a concrete `FilterStrategy`
  (`PreFilter` for narrow predicates, `PostFilter` for broad ones) from the
  estimate. `DEFAULT_PREFILTER_THRESHOLD` (`0.5`) is the documented default.

### Changed

- `FilterStrategy::Auto` is now resolved by the selector; `InFilter` remains
  reserved for a future graph-traversal consumer.

---

## [0.2.0] - 2026-06-05

The canonical filter evaluator lands. This turns the scaffold into a working
crate: one place that decides what an `iqdb_types::Filter` means, with
validation-on-construction and an infallible per-row boolean evaluator that
every metadata-aware index can delegate to.

### Added

- `FilterEvaluator` &mdash; `new(filter)` validates a filter once (nesting
  depth, `In` cardinality) and returns `IqdbError::InvalidFilter` on violation;
  `evaluate(Option<&Metadata>) -> bool` is then infallible and allocation-free
  on the per-row hot path. A `filter()` accessor borrows the validated tree for
  introspection.
- `FilterStrategy` &mdash; `#[non_exhaustive]` vocabulary enum
  (`PreFilter`, `PostFilter`, `InFilter`, `Auto`) for how an index applies a
  filter relative to its distance scan. Variants only; the selector lands with
  the first approximate-index consumer (see `dev/ROADMAP.md`).
- `MAX_FILTER_DEPTH` (`64`) and `MAX_IN_VALUES` (`1024`) &mdash; the public
  validation caps, exposed as `pub const` so higher layers can pre-check.
- `VERSION` &mdash; the crate's compile-time `CARGO_PKG_VERSION`.
- Closed-world null / absent-field semantics, pinned by the
  `tests/conformance.rs` integration suite and the `tests/properties.rs`
  property tests (determinism, boolean-algebra laws, De Morgan, closed world).
- `iqdb-types` `1.0.0` dependency for the shared `Filter` / `Metadata` /
  `Value` / `IqdbError` / `Result` vocabulary.
- A `criterion` benchmark for the validation and evaluation paths.

---

## [0.1.5] - 2026-06-05

Scaffold finalization &mdash; still no domain logic, this release tightens the bootstrap before the evaluator lands.

### Added

- Co-author **Matt Callahan** to the crate `authors`.

### Changed

- Pinned the MSRV to Rust `1.87` consistently across `Cargo.toml`, `clippy.toml`, the README badge, `dev/DIRECTIVES.md`, and the CI matrix (from `1.85`).

---

## [0.1.0] - 2026-05-30

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).
[Unreleased]: https://github.com/jamesgober/iqdb-filter/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/jamesgober/iqdb-filter/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/iqdb-filter/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/iqdb-filter/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/iqdb-filter/releases/tag/v0.1.0
