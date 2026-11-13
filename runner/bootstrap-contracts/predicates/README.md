# Bootstrap predicate datasets

`predicate-language.schema.json` is the single source for the supported
predicate operations, JSON data types, comparison and existence rules, and
representative type samples. `dataset-cases.json` separately holds authored
predicate cases for syntax, path, and evaluator behavior. The generator reads
the schema to build the type/operator matrix (including equality, ordering,
and `exists()` cases), combines it with `dataset-cases.json`, and writes
`dataset.json`. It also derives each case's `path` metadata from the first
predicate field reference. The sandboxes evaluate each case using its `json`
and `predicate` fields and compare the result to `expected`.

The generated dataset contains no AST predicate objects and no JSON-encoded
predicate expressions. The dataset shape is defined by
`dataset.schema.json`.

Generate or verify the checked-in dataset with:

```sh
make -C runner/bootstrap-contracts/predicates generate-dataset
make -C runner/bootstrap-contracts/predicates check-generated-dataset
```

The Rust evaluator in `src/evaluate.rs` is the predicate contract implementation. It
accepts predicate strings and evaluates them against the complete input JSON value. Expressions support dotted shorthand paths, JSON
scalar literals, comparison operators (`==`, `!=`, `>`, `>=`, `<`, `<=`), boolean
operators (`NOT`, `AND`, `OR`), parentheses, and `exists()`. Paths use native
dotted notation, including numeric array indexes: `.payload.items.0.name` and
`exists(.events.2.name)`. Bracketed JSON strings address keys that need quoting,
such as `.payload.items.0["a/b"]` or `.record["key.with.dots"]`. There is no
reserved `.facts` root. Missing paths compare as null, and `exists()` is false for
missing or null values, empty objects, and empty arrays.

## Coverage-driven sandbox

```sh
make -C runner/bootstrap-contracts/predicates check-predicate-contract
```

The Rust dataset executable runs all cases through the Rust evaluator under
`cargo llvm-cov`. Unit-test execution is excluded from this coverage report.
The check fails unless every executable evaluator line, function, and region is
covered. Rust coverage requires `cargo-llvm-cov` and the Rust LLVM tools. Run it
with `make -C runner/bootstrap-contracts/predicates check-rust-predicate-contract`.

The shared corpus is the conformance source for the Rust predicate contract.
The Python evaluator under `examples/python` is an example implementation; it
is not part of the Rust runner or the bootstrap predicate implementation.
