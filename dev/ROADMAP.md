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

**Scope note (anti-deferral rule).** The selectivity estimate here is
deliberately *structural*. An **index-backed** estimate (histograms / sketches)
and the inverted `MetadataIndex` it reads stay deferred below -- the threshold
the selector compares against is tunable precisely because the structural
estimate is coarse. This is the "pre/post + heuristic auto" scope, chosen over
building speculative index machinery with no consumer.

---

## Deferred until the first approximate-index consumer

Everything below was considered for the 0.x core and dropped because it has no
real consumer yet (`iqdb-flat`, the only metadata-aware index today, pre-filters
with a row scan). Each lands when `iqdb-hnsw` / `iqdb-ivf` start honouring
filters and the cost model actually changes -- not before.

### Inverted `MetadataIndex` (opt-in per field)

A per-field index from `Value` to a set of vector ids, so a selective predicate
can hand the index a candidate set instead of scanning every row. Per-field
opt-in (indexing every field inflates resident memory and write amplification),
and gated on a string-cap policy in `iqdb-types` for the unbounded metadata
surface.

### Index-backed selectivity estimate

A data-driven upgrade to the structural `estimate_selectivity` shipped in
v0.3.0: `selectivity_estimate(&Filter, &MetadataIndex) -> f64` reading real
sources -- per-field histograms for ordered domains, sketches (HyperLogLog /
count-min) for high-cardinality and membership domains. Only worth building once
the `MetadataIndex` above exists; until then the structural estimate plus a
tunable threshold is the honest surface.

### Filter pushdown into graph traversal

`FilterStrategy::InFilter`: prune HNSW / IVF branches that provably cannot
produce a surviving candidate, without collapsing recall. Requires
`MetadataIndex` co-design and a clear story for how pruning interacts with the
approximate index's recall guarantees.

---

## Toward 1.0

- **Index phase** -- the deferred items above (inverted `MetadataIndex`,
  index-backed selectivity, `InFilter` pushdown), in order, each with tests and
  benchmarks where it is a hot path, once a real approximate-index consumer
  drives them. Feature freeze declared at the end (no `todo!` /
  `unimplemented!`).
- **API freeze** -- public API frozen and recorded here; `cargo audit` +
  `cargo deny` clean.
- **Alpha / Beta / RC (0.6.x -> 0.9.x)** -- integrate against real consumers
  (MINOR-compatible additions only), broaden testing, final benchmarks, doc
  polish.

## v1.0.0 -- Stable

- [ ] Definition of Done (DIRECTIVES section 7) satisfied.
- [ ] Public API frozen until 2.0.
- [ ] Release note written; published to crates.io; tag pushed.

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
