# Batch execution bootstrap contract

This bootstrap contract exercises the batch runner's public functions with
inputs from `dataset.json`. The validator builds the manifest files required by
each case, invokes the public API, and compares its actual result with the
case's expectation.

Dataset coverage is the required coverage source. The scoped checker requires
100% dataset function, instantiation, line, and region coverage, and requires
every case to pin its observed 32-character fingerprint. Unit tests may cover
internal helper behavior that the public interface cannot expose, but their
coverage does not fill dataset gaps.

When a case needs an external condition, give it a setup fixture ID. The
validator resolves that ID to setup such as a controlled adapter executable,
then passes the case's actual input through the public batch API and checks the
returned result. Setup does not supply the expected result or call private
implementation helpers. Fixture controls stay outside `input`.

For consumer contracts, the Rust runner reads the manifest, starts the selected
isolation boundary, and executes each contract's declared language-adapter
command once with requests flattened from all of that contract's datasets.
The adapter measures their coverage together so the runner can enforce the
shared threshold on the contract-wide union. Dataset identity remains on each
request key, preserving each case's own result and coverage fingerprint.
Language adapters only execute validators and report results and coverage; they
do not read datasets, evaluate predicates, or provide isolation. That runtime
path is exercised when running consumer contracts. The batch bootstrap contract
focuses on public batch behavior. Unit tests are reserved for internal behaviors
the public batch interface cannot expose.

Datasets use format version 1 and may be JSON or YAML. The shared shape has
only a root `cases` list. A case is either one validator call or an ordered
`steps` list. Each call has real `input`, sibling `expected`, an optional
sibling `predicate`, and a required nonzero 32-character lowercase
`coverage_fingerprint`. `setup` is an optional fixture ID passed to the
validator as context; it does not replace or change `input`.

Steps execute in order. A run of adjacent steps with `parallel: true` is one
parallel group; the language adapter runs that group concurrently and returns
one keyed result for every step. Each step still owns its expected result,
predicate, setup ID, and fingerprint. There is no sequence-level input,
expected result, predicate, or fingerprint.

`expected` matches a recursive subset of the returned JSON object. A
`predicate` is complementary: when present, it is checked against the full
result after the expected subset matches. Validator errors are ordinary result
data under `{"error": "..."}`. Each fingerprint is the first 128 bits of
SHA-256 over sorted package-relative covered files and lines. Missing,
malformed, zero, and mismatched values fail. Repeated fingerprints are allowed
and do not add new covered lines.

```yaml
version: 1
cases:
  - id: direct-call
    input: {value: 4}
    expected: {accepted: true}
    predicate: .accepted == true
    setup: seed-fixture
    coverage_fingerprint: 0123456789abcdef0123456789abcdef
  - id: related-calls
    steps:
      - id: left
        input: {value: 2}
        expected: {result: 2}
        parallel: true
        coverage_fingerprint: 1123456789abcdef0123456789abcdef
      - id: right
        input: {value: 3}
        expected: {result: 3}
        parallel: true
        coverage_fingerprint: 2123456789abcdef0123456789abcdef
      - id: follow-up
        input: {value: 5}
        expected: {result: 5}
        coverage_fingerprint: 3123456789abcdef0123456789abcdef
```

Run only this contract's dataset and coverage checker from the Open Contract
repository root:

```sh
make check-batch-execution-contract
```

The same target is included in the Rust core check, alongside the other Rust
bootstrap contracts:

```sh
make check-rust-core-contracts
```
