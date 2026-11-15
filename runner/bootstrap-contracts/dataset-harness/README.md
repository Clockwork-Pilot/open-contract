# Bootstrap dataset harness

This crate is shared by the Rust bootstrap contracts. It loads JSON or YAML
datasets with the same versioned envelope and runs each case through the
contract's own validator:

```yaml
version: 1
cases:
  - id: example
    setup: true # optional; declares an ID-mapped test setup outside input
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
`setup` is optional and defaults to `false`; it indicates the validator must
perform a case-ID-mapped test setup before calling the public implementation.
Setup may prepare external conditions the dataset input cannot create, such as
environment values, temporary filesystem state, a fake executable, or a
controlled OS result. The validator must still pass the case's real `input` to
the public function and compare its actual result with `expected` or the result
predicate. Setup must not replace the public call, fabricate its result, or call
implementation internals to add lines to the dataset fingerprint. Keep setup
controls outside `input`; do not create placeholder cases or harness-only
operations.
Each case may declare a `coverage_fingerprint` pin for the implementation
lines it covers. A missing pin is treated as the all-zero sentinel
`00000000000000000000000000000000`. The shared coverage checker runs the
dataset once. During that run, this harness resets the LLVM counters before
each validator call and stores that case's profile snapshot in memory. Once
all cases have run, it restores the aggregate counters for the normal Rust
coverage report and writes the snapshots together as one artifact. The
checker calculates per-case fingerprints from those snapshots after the
dataset run with the Rust toolchain's `llvm-profdata` and `llvm-cov`; it does
not rerun the contract for each case or edit datasets. The checker processes
case profiles with a bounded worker pool. Set `OPEN_CONTRACT_COVERAGE_WORKERS`
to a positive integer to choose the pool size; by default it uses up to four
workers, limited by the available CPU count.

Fingerprints include covered executable source lines under the contract's
`src` directory. A missing, malformed, zero, or mismatched pin fails the
check and reports the runtime fingerprint. Copy the reported 32-character
lowercase hexadecimal value into the matching case, then rerun the check. A
pin is optional while authoring a case, but required for a passing coverage
check.
The format is described by [`dataset.schema.json`](dataset.schema.json).
Check this shared harness crate from the Open Contract repository root:

```sh
make check-bootstrap-dataset-harness
```
