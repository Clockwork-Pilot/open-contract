# Open Contract bootstrap contracts

Bootstrap contracts test the Rust runner and its components. Their datasets
should exercise each component through the inputs and results available at its
public boundary. A dataset case carries functional input and the expected
observable result: exact output, predicate, expected error, or a sequence of
public calls.

Dataset cases must exercise each component through its public interface with
real input and observable expected results. A validator may use case-specific
setup before the public call when the input alone cannot create an external
condition, such as a temporary filesystem state, environment value, fake
executable, or controlled OS result. Declare this with an optional fixture ID
in `setup`, outside `input`. The validator resolves that ID, prepares its
fixture, and passes the dataset's actual `input` unchanged to the public
function before checking its result. Setup must not replace the call, fabricate
its result, or invoke implementation internals to add lines to dataset coverage. Keep
fixture controls outside `input`; do not add harness-only operations or
placeholder inputs.

Use ordinary Rust unit tests for implementation details the public call cannot
exercise. Keep those helper calls in the unit-test path, separate from dataset
case setup and dataset fingerprints.

The bootstrap coverage check runs each package's dataset executable and
requires 100% implementation function, instantiation, line, and LLVM region
coverage from the dataset. Unit-test coverage does not contribute to this
requirement; use unit tests for implementation details the public dataset
input cannot exercise. The check also requires every case fingerprint to
match the covered source text observed for that case.

Open Contract datasets use version 1. Every case and every sequence step must
declare a matching, nonzero `coverage_fingerprint`. A missing, malformed,
zero, or stale fingerprint fails. Refresh it when the execution's covered
source text changes. The hash includes each covered file's contract-relative
path and the exact text of its covered lines, in source order; it omits line
numbers. Adding unrelated lines above covered code therefore does not
invalidate pins. Matching fingerprints may repeat across distinct cases: that
means the cases cover the same source text, not that they test the same
behavior. Repeated values add no new line coverage, so use the coverage report
to assess the dataset's total coverage.

Per-case fingerprints are calculated with the Rust toolchain's `llvm-profdata`
and `llvm-cov`. The checker processes separate case profiles in a bounded
worker pool. Set `OPEN_CONTRACT_COVERAGE_WORKERS` to a positive integer to
choose the pool size; by default it uses up to four workers, limited by the
available CPU count. Keep the Rust toolchain consistent when checking and
refreshing fingerprints because LLVM versions can map source coverage to
lines differently. The repository pins Rust 1.92.0 and its
`llvm-tools-preview` component for this reason. Coverage outputs and Cargo
targets are isolated under the ignored `.coverage/bootstrap-contracts/`
directory and refreshed at the start of each check.

Bootstrap packages use the shared `bootstrap-dataset-harness` harness.
Every bootstrap dataset has `version: 1` and `cases`, with each case carrying
`id` and `input`, plus `expected`, a top-level result `predicate`, or both.
`expected` and result `predicate` are siblings, following the Open Contract
case convention. The `input` and `expected` values remain specific to the
public component being exercised.
The shared harness reads either JSON (`.json`) or YAML (`.yaml`/`.yml`) and
owns envelope checks, ordered case execution, and case-qualified errors.
Each case should declare a `coverage_fingerprint`. The coverage checker runs
each validator case in isolation, calculates its covered-source-text
fingerprint from the runtime report, and reports the observed value for missing or mismatched
pins. The check fails until the implementor copies the reported fingerprint
into the dataset and reruns it. The checker never edits datasets.
An omitted fingerprint is reported as the all-zero sentinel
`00000000000000000000000000000000`.
Open Contract consumer datasets continue to follow the Open Contract schema,
including their required coverage fingerprints.

Run all bootstrap contract checks from the Open Contract repository root:

```sh
make check-bootstrap-contracts
```

Run an individual package check with its corresponding target:

- `make check-predicate-contract`
- `make check-manifest-contract`
- `make check-isolation-contract`
- `make check-batch-execution-contract`
- `make check-verification-contract`
- `make check-error-handling-contract`
- `make check-dataset-generation-contract`
- `make check-cli-contract`
- `make check-cli-contract`
- `make check-bootstrap-dataset-harness` (shared harness crate)
