# Bootstrap predicate datasets

`predicate-language.schema.json` is the single source for supported predicate
operations, JSON data types, comparison and existence rules, and representative
type samples. `dataset-cases.json` holds authored cases in the same version 1
envelope used by the other bootstrap contracts. The generator builds the
type/operator matrix from the schema, combines it with authored cases, and
writes `dataset.json`. Each case passes a JSON value and predicate expression
to the public evaluator and asserts the returned boolean.

The generated dataset contains no AST predicate objects and no JSON-encoded
predicate expressions. Its versioned shape is defined by `dataset.schema.json`.

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

Run only this contract from the Open Contract repository root:

```sh
make check-predicate-contract
```

The shared corpus is the conformance source for the Rust predicate contract.
The standalone [Python predicate Open Contract example](../../../examples/python/predicate/README.md)
is an independent implementation; it is not part of the Rust runner or the
bootstrap predicate implementation.
