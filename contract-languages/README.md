# Contract language adapters

Language adapters receive flattened calls from the Rust runner and invoke the
contract's public validator with the real `input` plus a context object. The
context contains `case_id`, optional `step_id`, optional `setup` fixture ID,
and the `parallel` marker. Adapters execute unmarked calls and steps in order;
adjacent `parallel: true` steps in one case form a concurrent group. They
return one raw output or error per request, keyed by case and step, plus
language-native coverage data. They do not interpret setup IDs or replace
public calls with harness operations. The Rust runner checks each result
against that call's sibling `expected` and optional `predicate` after execution.

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
