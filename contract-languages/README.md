# Contract language adapters

Language adapters receive flattened calls from the Rust runner and invoke the
contract's public validator with the real `input` plus a context object. The
context contains `case_id`, optional `step_id`, optional `setup` fixture ID,
and the `parallel` marker. Adapters execute unmarked calls and steps in order;
adjacent `parallel: true` steps in one case form a concurrent group. They
return one raw output or error per request, keyed by case and step, plus native
coverage, normalized `coverage_lines`, and adapter-reported `coverage_entities`.
Entities carry a relative source path, metric, status, and the location/count
precision supported by that adapter. The runner validates these payloads against
`schemas/adapter-coverage.v1.schema.json` before calculating thresholds. They do
not interpret setup IDs or replace
public calls with harness operations. The Rust runner checks each result
against that call's sibling `expected` and optional `predicate` after execution.

The runner may set `coverage_fingerprints` to `false` in the batch request for
diagnostic runs. Adapters then omit per-call `coverage_scopes`, while still
returning aggregate `coverage` and `coverage_lines`; the runner keeps aggregate
coverage-threshold checks active. If the field is absent or `true`, adapters
collect per-call scopes as usual.

Available adapters:

- [`javascript`](javascript/README.md)
- [`python`](python/README.md)
- [`rust`](rust/README.md)

The Rust runner can render an offline HTML report from these adapter-provided
entities with `run-manifest --coverage-html <directory>`. It reports source
lines and the line, statement, function, branch, region, and instantiation data
provided by each adapter. Metrics or entity locations absent from adapter data
are labeled unavailable; the runner does not derive them from another metric.
The report includes each declared dataset, while entity data is a contract-wide
coverage union because adapters currently execute all of a contract's datasets
in one batch.

Run every language adapter check and end-to-end example with
`make check-language-contract-examples` from the Open Contract root. These
examples all use the Rust runner; Python and JavaScript provide validator
adapters. The Python packages are in [`examples/python`](../examples/python),
the JavaScript example is in [`examples/javascript`](../examples/javascript),
and nested Rust contracts are in [`examples/rust`](../examples/rust).
