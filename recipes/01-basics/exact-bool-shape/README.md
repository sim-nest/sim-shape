# Exact Boolean Shape Recipe

This runnable recipe builds an exact expression shape for `true`, checks it
against both `true` and `false`, parses a capture shape from shape grammar, and
prints the resulting binding.

Run it from the repository root:

```bash
cargo run --manifest-path recipes/01-basics/exact-bool-shape/Cargo.toml
```

The output shows the accepted positive match, the rejected negative match, and
the expression captured by the parsed shape.
