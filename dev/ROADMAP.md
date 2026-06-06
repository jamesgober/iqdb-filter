# iqdb-filter -- Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

---

## v0.1.0 -- Scaffold (DONE)

Compiles, CI green, structure correct, no domain logic.

- [x] Manifest, README, CHANGELOG, REPS, license, CI, lints in place.
- [x] API surface sketched in `docs/API.md`.

---

## v0.2.0 -- Canonical filter evaluator (DONE)

The hard part of the crate's core: one evaluator that decides what a `Filter`
means, shared by every consumer so the semantics cannot drift.

- [x] `FilterEvaluator` -- validation-on-construction (depth + `In` caps,
      iterative so `new` cannot overflow) and an infallible, allocation-free
      per-row `evaluate`.
- [x] Closed-world null / absent-field semantics, including the
      `Neq(absent)` / `Not(Eq(absent))` distinction.
- [x] `FilterStrategy` vocabulary enum (`#[non_exhaustive]`, variants only).
- [x] `MAX_FILTER_DEPTH` / `MAX_IN_VALUES` public caps; `VERSION`.
- [x] Every public item has rustdoc + a runnable example.
- [x] Core invariants property-tested (`tests/properties.rs`) and pinned as
      conformance integration tests (`tests/conformance.rs`).
- [x] `criterion` bench on the validation and evaluation paths.

**Deferral recorded (anti-deferral rule).** The original plan paired the
evaluator with a *selectivity estimate* in this phase. It moved to v0.3.0
below, because bundling it with the evaluator added surface no consumer
exercised yet. A placeholder estimate ("0.5 always") would have been worse than
none -- it would teach a selector to make confident wrong choices.

---

## v0.3.0 -- Strategy selection (DONE)

The `FilterStrategy` vocabulary becomes actionable, without an inverted index.

- [x] `FilterEvaluator::prefilter` / `postfilter` -- lazy, allocation-free scan
      adapters over `(key, metadata)` pairs, realising the `PreFilter` /
      `PostFilter` shapes. Shaped to the real `iqdb-flat` consumption pattern.
- [x] `estimate_selectivity(&FilterEvaluator) -> f64` -- a **structural**
      estimate (filter shape only, no data) in `[0.0, 1.0]`. Takes a validated
      evaluator, so the walk is depth-bounded.
- [x] `choose_strategy` (Tier 1) + `StrategySelector` (Tier 2, tunable
      `prefilter_threshold`, immutable builder) resolving `PreFilter` /
      `PostFilter`; `DEFAULT_PREFILTER_THRESHOLD = 0.5`.
- [x] New surface property-tested (selectivity is always a probability;
      `prefilter` matches the manual filter; the selector never yields
      `Auto` / `InFilter`) and benchmarked.

**Scope note.** The selectivity estimate in this phase is deliberately
*structural*; the **index-backed** estimate and the inverted `MetadataIndex` it
reads were deferred from v0.3.0 and landed in v0.4.0 below.

---

## v0.4.0 -- Inverted metadata index (DONE)

The opt-in per-field index and the data-backed selectivity estimate it enables.

- [x] `MetadataIndex<K>` -- `build(fields, records)` indexes only the named
      fields (per-field opt-in, so write/memory cost is paid only where it
      buys query speed). `candidates(&FilterEvaluator) -> Option<Vec<K>>`
      resolves `Eq` / `In` / `And` / `Or` over `String` / `Int` / `Bool` /
      `Null`, returning a **superset** of true matches; `None` for the cases an
      inverted index cannot bound (ranges, `Neq`, `Not`, `Float`, non-indexed
      fields). Float is excluded by design: IEEE equality would risk a dropped
      match.
- [x] `MetadataIndex::estimate_selectivity` -- real `matches / total` counts for
      resolvable leaves, structural fallback elsewhere.
- [x] `StrategySelector::choose_with_index` -- the pre/post decision driven by
      the index-backed estimate.
- [x] Superset contract property-tested (every true match is a candidate);
      selectivity-is-a-probability property-tested; `candidates` and `build`
      benchmarked.

**Scope note.** Histogram / sketch estimators (HyperLogLog, count-min) over the
index are a future refinement, not required: exact posting counts already give
an accurate equality/membership estimate. The string-cap policy in `iqdb-types`
(audit item for unbounded metadata) remains the one open dependency before the
index is hardened for hostile, high-cardinality input.

---

## v0.5.0 -- Hardening & API freeze (DONE)

No new public surface; prove the existing one is sufficient and robust, then
commit to it.

- [x] Consumer-simulation suite -- a filtered top-`k` searcher built only on the
      public API, asserting the index-accelerated path equals a full scan for
      every filter shape.
- [x] Fuzz targets (`fuzz/`): `robustness` (validator/evaluator never panic) and
      `superset` (the index contract holds on unbounded input). Wired into CI.
- [x] **Public API frozen** (recorded below). Only additive (MINOR) changes
      until 2.0.

### Frozen public surface (v0.5.0)

- Types: `FilterEvaluator`, `FilterStrategy`, `StrategySelector`,
  `MetadataIndex<K>`.
- Consts: `MAX_FILTER_DEPTH`, `MAX_IN_VALUES`, `DEFAULT_PREFILTER_THRESHOLD`,
  `VERSION`.
- Functions: `estimate_selectivity`, `choose_strategy`.
- `FilterEvaluator`: `new`, `evaluate`, `filter`, `prefilter`, `postfilter`.
- `StrategySelector`: `new`, `with_prefilter_threshold`, `prefilter_threshold`,
  `choose`, `choose_with_index`.
- `MetadataIndex`: `build`, `candidates`, `estimate_selectivity`, `len`,
  `is_empty`, `is_indexed`, `indexed_fields`.

Additive-only seams kept open for the deferred work: `FilterStrategy` is
`#[non_exhaustive]` (so `InFilter` pushdown needs no new variant), and the
selectivity / candidate surfaces are separate functions (so an index-backed or
pushdown path is a new method, never a signature change).

---

## Deferred until the first approximate-index consumer

What remains needs a real approximate index (`iqdb-hnsw` / `iqdb-ivf`) honouring
filters before it can be built and validated honestly -- not before.

### Filter pushdown into graph traversal

`FilterStrategy::InFilter`: prune HNSW / IVF branches that provably cannot
produce a surviving candidate, without collapsing recall. Requires
`MetadataIndex` co-design and a clear story for how pruning interacts with the
approximate index's recall guarantees.

---

## Toward 1.0

- **API freeze** -- done in v0.5.0 (frozen surface recorded above);
  `cargo audit` + `cargo deny` clean.
- **RC (0.6.x -> 0.9.x)** -- integrate against the first real consumer
  (`iqdb-flat`), MINOR-compatible additions only, final benchmarks, doc polish.
  This is the remaining gate to 1.0: the API is frozen and hardened, so what's
  left is soak time against a live consumer.
- **`InFilter` pushdown** -- the one deferred feature, built when an approximate
  index drives it. Additive (`FilterStrategy` is `#[non_exhaustive]`), so per
  SemVer it can ship pre- or post-1.0 in a MINOR release without waiting.

## v1.0.0 -- Stable (DONE)

- [x] Definition of Done (DIRECTIVES section 7) satisfied. (`loom` is N/A: the
      crate has no shared-state or lock-free paths -- `FilterEvaluator` and
      `MetadataIndex` are immutable and `Sync` by construction.)
- [x] Public API frozen until 2.0 (committed under SemVer for the 1.x series).
- [x] Release note written (`docs/release/v1.0.0.md`). Publishing to crates.io
      and the tag push are handled by the maintainer.

The crate is feature-complete for 1.0; `InFilter` pushdown is the lone additive
follow-up, to land in a 1.x MINOR when a graph-index consumer drives it.

---

## Out of scope for 1.0

- The `Filter` type itself -- defined in `iqdb-types`.
- A full query language -- expression evaluation only.
- A different evaluator algorithm. The recursive walker is simple, fast on the
  bounded-depth filters this crate accepts, and correct. Replacing it with a
  bytecode compiler or JIT is speculative until a profile shows it costs
  anything.
- `Filter` rewriting / canonicalisation (constant-fold, push `Not` down,
  reorder `And` children by selectivity) -- all depend on the deferred
  selectivity estimates; premature without them.
