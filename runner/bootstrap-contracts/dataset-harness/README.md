# Bootstrap dataset harness

This crate is shared by the Rust bootstrap contracts. It loads JSON or YAML
datasets with the same versioned envelope and runs each case through the
contract's own validator:

```yaml
version: 1
cases:
  - id: example
    setup: example-fixture # optional fixture ID, separate from input
    input: {value: 1}
    coverage_fingerprint: 0123456789abcdef0123456789abcdef
    expected: {value: 1}
    predicate: ".value == 1" # optional result assertion; sits beside expected
```

`input` and `expected` are JSON-shaped values whose meaning belongs to the
contract under test. A case must provide `expected`, a case-level result
`predicate`, or both. If the public function under test accepts a predicate
expression as an input, that expression belongs inside `input`; the sibling
case-level `predicate` describes a result assertion. The harness owns loading,
envelope validation, case order, and case-qualified errors; each bootstrap
contract owns behavior assertions.
`setup` is an optional fixture ID. The validator resolves that ID and prepares
external conditions the real input cannot create, such as environment values,
temporary filesystem state, a fake executable, or a controlled OS result. It
must pass the case's `input` unchanged to the public function and compare the
actual result with `expected` or the result predicate. Setup must not replace
the public call, fabricate its result, or call implementation internals to add
lines to the dataset fingerprint. Keep fixture state outside `input`; do not
create placeholder cases or harness-only operations.
Each case may declare a `coverage_fingerprint` pin for the implementation
lines it covers. A missing pin is treated as the all-zero sentinel
`00000000000000000000000000000000`. The shared coverage checker runs the
dataset once. Its test-only dataset entrypoint resets LLVM counters before
each validator call and stores that case's profile snapshot in memory. Once
all cases have run, the test entrypoint restores aggregate counters for the
normal Rust coverage report and writes the snapshots together as one artifact. The
checker calculates per-case fingerprints from those snapshots after the
dataset run with the Rust toolchain's `llvm-profdata` and `llvm-cov`; it does
not rerun the contract for each case or edit datasets. The checker processes
case profiles with a bounded worker pool. Set `OPEN_CONTRACT_COVERAGE_WORKERS`
to a positive integer to choose the pool size; by default it uses up to four
workers, limited by the available CPU count.

Fingerprints hash covered source text under the contract's `src` directory:
the relative file path and exact covered line contents in source order, without
line numbers. Adding unrelated lines above covered code leaves the hash stable;
editing covered text changes it. A missing, malformed, zero, or mismatched pin fails the
check and reports the runtime fingerprint. Copy the reported 32-character
lowercase hexadecimal value into the matching case, then rerun the check. A
pin is optional while authoring a case, but required for a passing coverage
check.
The format is described by [`dataset.schema.json`](dataset.schema.json). The
harness exercises its own public loader and dataset runner through
[`dataset.json`](dataset.json); its validator supplies real nested dataset
documents in `input` and checks the returned behavior. LLVM collection lives
in test support and writes generated profiles under the Open Contract root
`.coverage/bootstrap-contracts/`. The package check requires 100% dataset
function, instantiation, line, and region coverage plus matching case
fingerprints.

Check this shared harness crate from the Open Contract repository root:

```sh
make check-bootstrap-dataset-harness
```
