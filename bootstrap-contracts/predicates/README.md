# Bootstrap predicate datasets

`generation-rules.json` is the source for `dataset.json`. Its case rules provide
the input JSON, string predicate, and expected boolean result. The generator
derives each output case's `path` from the first predicate reference, then emits
one uniform list of cases. The dataset contains no AST predicate objects and no
JSON-encoded predicate expressions. The predicate schema is in
`predicate-language.schema.json`; the generated dataset shape is in
`dataset.schema.json`.

Generate or verify the checked-in dataset with:

```sh
make -C bootstrap-contracts/predicates generate-dataset
make -C bootstrap-contracts/predicates check-generated-dataset
```

The Python and Rust evaluators accept predicate strings and evaluate them against
the complete input JSON value. Expressions support dotted shorthand paths, JSON
scalar literals, comparison operators (`==`, `!=`, `>`, `>=`, `<`, `<=`), boolean
operators (`NOT`, `AND`, `OR`), parentheses, and `exists()`. Paths use native
dotted notation, including numeric array indexes: `.payload.items.0.name` and
`exists(.events.2.name)`. Bracketed JSON strings address keys that need quoting,
such as `.payload.items.0["a/b"]` or `.record["key.with.dots"]`. There is no
reserved `.facts` root. Missing paths compare as null, and `exists()` is false for
missing or null values, empty objects, and empty arrays.

## Coverage-driven sandbox

```sh
make -C bootstrap-contracts/predicates check-predicate-contract
```

The shared dataset runs under both language sandboxes. The Python sandbox traces
the evaluator statements, and the Rust sandbox uses `cargo llvm-cov`; both checks
fail unless every executable evaluator line is covered. Rust coverage requires
`cargo-llvm-cov` and the Rust LLVM tools. Run one language with
`check-python-predicate-contract` or `check-rust-predicate-contract`.

The implementations are standalone and have no runtime dependency on `/a2a` or
`knowledge_tool`. Python and Rust remain grouped under this dataset so their
string-language behavior stays synchronized.

## Python package

Build the package with:

```sh
python3 -m pip wheel --no-deps bootstrap-contracts/predicates/python
```
