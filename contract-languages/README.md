# Contract language adapters

Language adapters receive the flattened request batch from the Rust runner and
return keyed validator results plus language-native coverage data. They invoke
the contract's validator; the runner owns isolation, expected-value checks, and
predicates.

Available adapters:

- [`javascript`](javascript/README.md)
- [`python`](python/README.md)
- [`rust`](rust/README.md)

Run every language adapter check and end-to-end example with
`make check-language-contract-examples` from the Open Contract root. This
includes the JavaScript and Python contract examples and nested Rust examples.

The JavaScript example is in [`examples/javascript`](../examples/javascript),
the Python greeting and predicate examples are in
[`examples/python/greeting`](../examples/python/greeting/README.md) and
[`examples/python/predicate`](../examples/python/predicate/README.md), and the
Rust nested-contract example is in [`examples/rust`](../examples/rust/README.md).
