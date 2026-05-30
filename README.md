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
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>iqdb-filter</strong> combines metadata filtering with vector similarity efficiently. The hard part it solves is applying a filter during an HNSW or IVF traversal without destroying recall.
    </p>
    <p>
        It evaluates the filter language defined in `iqdb-types`, estimates selectivity, and chooses among pre-, post-, and in-traversal strategies automatically.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). Filtered search done right. Selectivity-aware strategy selection.
    </p>
    <blockquote>
        <strong>Status: pre-1.0, in active development.</strong> The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

<h2>What it does</h2>

- **Filter evaluation** &mdash; evaluate the `Filter` expression language from iqdb-types against metadata
- **Three strategies** &mdash; pre-filter, post-filter, in-traversal filtering
- **Auto strategy** &mdash; pick the strategy from a selectivity estimate, with manual override
- **Metadata index** &mdash; optional per-field inverted index for fast, selective pre-filtering
- **First-class** &mdash; filtered search treated as a core concern, not an afterthought


<br>

## Installation

```toml
[dependencies]
iqdb-filter = "0.1"
```

<br>

## Status

This is the <code>v0.1.0</code> scaffold: structure, tooling, and quality gates are in place; the implementation lands across the 0.x series per the <a href="./dev/ROADMAP.md"><code>ROADMAP</code></a> and <a href="./docs/API.md"><code>docs/API.md</code></a>.

<hr>
<br>

## Where It Fits

`iqdb-filter` is consumed by the graph and clustered indexes. It depends on:

- `iqdb-types` &mdash; the `Filter`, `Metadata`, and `Value` types
- `iqdb-hnsw` / `iqdb-ivf` &mdash; consume this for filtered traversal
- `iqdb` &mdash; exposes filtered search to users

It depends only on `iqdb-types`, so it is unblocked today.

<br>

## Contributing

See <a href="./dev/DIRECTIVES.md"><code>dev/DIRECTIVES.md</code></a> for engineering standards and the definition of done. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

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
