# Contributing

Thanks for your interest in SIM. This repo is one crate group in a constellation
of repos that build together; contributions of all sizes are welcome.

## Building and testing

This repo is self-contained and builds against the published SIM crates on
crates.io. CI installs the channel named by `rust-toolchain.toml`. The
generated-doc check delegates to the shared `sim-tooling` encoder; CI checks out
`sim-nest/sim-tooling` and sets `SIMDOC_TOOLING_MANIFEST`, while local runs can
use either a sibling `sim-tooling` checkout or the same environment variable.

- Clone this repo and run `cargo build` and `cargo test --workspace`.
- Cross-repo dependencies resolve from crates.io; dependencies within this repo
  resolve locally.

## What a pull request must pass

Every PR runs these gates in CI, and they must be green before merge:

- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo test --workspace --all-features`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- `cargo run -p xtask -- simdoc --check`

Please keep source and Markdown ASCII-only, and add or update tests for behavior
you change. Public APIs carry `#![deny(missing_docs)]`; document new public items.

## Sign your work (DCO)

We use the Developer Certificate of Origin, not a CLA. Add a `Signed-off-by` line
to each commit certifying you wrote the change or have the right to submit it:

```
git commit -s -m "your message"
```

This adds `Signed-off-by: Your Name <you@example.com>`. That is all we need; there
is no copyright-assignment agreement to sign.

## License

By contributing you agree that your contributions are licensed under the
repository's MPL-2.0 license (see `LICENSE`).

## Filing issues

Use the issue templates. A small reproducible example beats a long description.
Security-sensitive reports go through `SECURITY.md`, not public issues.
