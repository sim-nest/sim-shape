# Shape Grammar Contract Recipe

This runnable recipe lowers a list Shape into a codec-neutral grammar graph,
renders JSON Schema, JSON GBNF, and Lisp grammar text, then checks one accepted
and one rejected JSON value with `grammar_check`.

Run it from the repository root:

```bash
cargo run --manifest-path recipes/01-basics/shape-grammar-contract/Cargo.toml
```

The output reports the graph and renderer checks, followed by the positive and
negative grammar-check results.
