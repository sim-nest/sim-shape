# sim-shape

sim-shape tells you whether a value or expression fits a pattern you describe --
and reports what it matched when it does.

SIM is a small Rust protocol kernel plus loadable libraries (not a Lisp
runtime); the `sim` CLI installs with `cargo install sim-run`, and sim-say is
the full walkthrough. sim-shape is a library.

## Example

```bash
cargo add sim-shape
```

```rust
use std::sync::Arc;
use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy};
use sim_shape::{ExactExprShape, Shape};

let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
let shape = ExactExprShape::new(Expr::Bool(true));
assert!(shape.check_expr(&mut cx, &Expr::Bool(true)).unwrap().accepted);
assert!(!shape.check_expr(&mut cx, &Expr::Bool(false)).unwrap().accepted);
```

`ExactExprShape` accepts only the expression it was built from: the first
`check_expr` is `accepted`, the second is not. (From the passing doctest in
`src/primitives/atomic.rs:319`.)

## How it works

The kernel owns the open
`Shape` protocol; this crate supplies the concrete shapes and the one shared
match, bind, and dispatch engine built on it. A shape both checks an expression
or value and reports what it captured, so the same engine serves parsing,
checking, binding, dispatch, codec grammar, lambda locals, and overload
selection.

The engine spans atomic shapes and their combinators and object-grammar parsers
(primitives), boolean and collection algebra (algebra), comparison and
subsumption reasoning (compare), citizen integration that registers shapes as
constructible objects (citizen), the callable shape object with overload
selection (functions), and match-extension hooks (hooks).

## Crates

- `sim-shape` -- concrete `Shape` implementations and the shared match, bind,
  and dispatch engine: primitive shapes, shape algebra, comparison and
  subsumption, citizen integration, callable shape objects, and match hooks.

## Validation

This repo validates from a single clone against the SIM crates published on
crates.io. CI installs the channel named by `rust-toolchain.toml` instead of a
floating stable toolchain. The generated-doc check delegates to the shared
`sim-tooling` encoder; CI checks out `sim-nest/sim-tooling` and sets
`SIMDOC_TOOLING_MANIFEST`, while local runs can use either a sibling
`sim-tooling` checkout or the same environment variable.

```bash
cargo fmt --all --check
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo run -p xtask -- simdoc --check
```

## Documentation Lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagrams: `docs/diagrams/src/` and `docs/diagrams/generated/`

The same command writes split contract files under `docs/generated/`. Everything
under `docs/` is generated; do not hand-edit it.

### Rustdoc conventions

Public API documentation in `src/` follows one house style:

- Every public item opens with a one-line summary sentence, then context.
- The kernel defines the `Shape` protocol; this crate implements it as the one
  shared engine, so each surface (primitives, algebra, compare, citizen,
  functions, hooks) is framed by its role in that engine.
- The first-reach types carry a `# Examples` doctest that compiles and passes.
- Cross-reference with intra-doc links, and link back to this README rather than
  restating it.

The public API is documentation-gated: `lib.rs` denies `missing_docs`, so every
public item, field, and variant must be documented for the crate to build.

### Examples and recipes

The crate's compact examples are rustdoc doctests. The `recipes/` lane carries a
runnable Rust recipe that checks an exact boolean shape from a standalone clone:

```bash
cargo run --manifest-path recipes/01-basics/exact-bool-shape/Cargo.toml
```
