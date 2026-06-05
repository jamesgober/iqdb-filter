<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>iqdb-filter</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

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
[Unreleased]: https://github.com/jamesgober/iqdb-filter/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/iqdb-filter/compare/v0.1.5...v0.2.0
[0.1.5]: https://github.com/jamesgober/iqdb-filter/compare/v0.1.0...v0.1.5
[0.1.0]: https://github.com/jamesgober/iqdb-filter/releases/tag/v0.1.0
