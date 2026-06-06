# iqdb-filter fuzz targets

Cargo-fuzz targets for `iqdb-filter`. This is a standalone crate (its own
workspace), excluded from the normal build; it requires a **nightly** toolchain
and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

```sh
cargo install cargo-fuzz
```

## Targets

| Target | What it checks |
|---|---|
| `robustness` | `FilterEvaluator::new` returns a typed `Result` (never panics or overflows) on any filter, and `evaluate` / `prefilter` never panic on any metadata — including `NaN`/`Inf` floats and absent fields. |
| `superset` | `MetadataIndex::candidates` upholds its contract: when it returns `Some(set)`, every record the evaluator accepts is in `set` (no dropped matches). The unbounded-input counterpart of the property test in `tests/properties.rs`. |

Both targets build their inputs from `arbitrary`-derived `Spec*` mirrors of the
`iqdb-types` `Filter` / `Value` / `Metadata` types (see `src/lib.rs`), since the
foreign types cannot derive `Arbitrary` directly. Conversion caps nesting depth
so building the tree cannot itself overflow; the validator does the real
depth/cardinality rejection.

## Run

From the crate root (the parent of this directory):

```sh
# Run until the first failure (or Ctrl-C).
cargo +nightly fuzz run robustness
cargo +nightly fuzz run superset

# Time-boxed run (CI-friendly).
cargo +nightly fuzz run superset -- -max_total_time=60

# Just build the targets (no fuzzing) — what CI does to prevent bitrot.
cargo +nightly fuzz build
```

Findings are written to `fuzz/artifacts/<target>/`; the evolving corpus lives in
`fuzz/corpus/<target>/`. Both are git-ignored.
