# Open Contract

Open Contract has one generic runner, written in Rust. It plans a manifest,
starts one filesystem isolation boundary, runs the dataset batch and declared
nested contracts, then verifies outputs and predicates after execution.

## Repository layout

```text
runner/rust/                         Generic runner and application composition
runner/tools/                        Developer tooling for bootstrap dataset coverage
contract-languages/python/           Python validator adapter and support
contract-languages/javascript/        JavaScript validator adapter and support
runner/bootstrap-contracts/          Open Contract's own contract packages
  manifest/                          Manifest planning contract and Rust code
  isolation/                         Filesystem isolation contract and Rust code
  batch-execution/                    Dataset, step, and nested execution contract
  verification/                      Expected output and predicate matching contract
  predicates/                        Predicate semantics and conformance corpus
bin/                                  Low-level isolation launch helpers
```

The Python and JavaScript directories provide language-specific validator
execution and native coverage collection. They do not select isolation, plan
contracts, traverse datasets, match expectations, or evaluate predicates.

Each Rust bootstrap package is stored next to the contract dataset that
exercises it. The Rust application depends on those packages; the bootstrap
contracts are not placeholder copies of code hidden in the application.

## Why contracts

A consumer dataset case describes one call to the contract's public
interface. Its `input` is passed unchanged to the public validator; sibling
`expected` and optional `predicate` describe the result the validator actually
returns. The language adapter invokes the validator with that input and
returns its result. The Rust runner checks the result against `expected` and
then evaluates the optional predicate. A returned error is part of that
observable result and is checked in the same way.

Dataset cases do not define fixtures, harness operations, or alternate
execution layers. When a test harness needs to prepare an external condition,
keep setup in validator-side code selected by the case ID and run it before
the public call. Then make the same public call with the dataset's actual
input and check the result it produces. Setup must not fabricate the result
or bypass the public interface. Open Contract runs each contract inside the
filesystem boundary declared by its manifest.

Dataset cases are the primary behavior checks because they describe what a
contract user can provide and observe. Use ordinary unit tests for internal
conditions the public interface cannot expose; those tests do not add dataset
coverage. Unit tests may reuse dataset inputs as regression examples.

## Run contracts

Select the isolation backend when invoking the Rust runner:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- \
  run-manifest runner/bootstrap-contracts/batch-execution/contract.yaml \
  --isolation landlock
```

The manifest describes package-relative logical read and write paths. The
runner stages contract files under a fresh root for each check, maps declared
paths into that tree, and starts the chosen backend at runtime. Contract code
must not rely on fixed absolute paths.

The root contract starts isolation once. Its dataset and declared nested
contracts execute within that boundary. Running a nested contract directly
starts a separate boundary. Nested coverage is combined into the root coverage
report while each contract's case results remain distinct.

## Isolation backends

- **Bubblewrap** provides a restricted filesystem view and namespace
  isolation. The runner limits mounts to contract files and required runtime
  resources.
- **Landlock** restricts filesystem access to allowed paths. Paths remain
  visible, and Landlock does not restrict networking.

Isolation startup failures are reported; the runner does not continue the
contract outside the requested boundary. `none` is available for trusted
debugging only.

## Verification and coverage

Language adapters receive batches of inputs and return serialized outputs,
errors, and native coverage. The Rust verifier compares outputs with exact
expected values or evaluates the shared predicate language after the batch
returns. Contracts are not assumed to be network services.

Bootstrap Rust packages run their dataset executable with
`runner/tools/check_bootstrap_contract_coverage.py`. The dataset must cover
100% of implementation lines and LLVM regions. Unit-test coverage is excluded
from this requirement; use unit tests for implementation details the public
dataset interface cannot exercise.

After running the contract checks, generate HTML reports for all bootstrap
contracts with:

```sh
make bootstrap-contract-coverage-html
```

Reports are written to `.coverage/bootstrap-contracts/<contract-name>/html/` at
the Open Contract repository root. The directory is ignored by Git. Each report
excludes sibling bootstrap crates used as dependencies; the shared
`dataset-harness` crate has no separate report.

Every Open Contract dataset case and step must declare a matching,
nonzero `coverage_fingerprint`. It records the covered package-relative source
files and lines for that execution. Missing, malformed, zero, or stale values
fail the case and must be refreshed when its observed coverage changes.
Coverage checks use the repository-pinned Rust toolchain because LLVM versions
can map source coverage to lines differently. Rustup installs the configured
toolchain and `llvm-tools-preview` component when they are missing.
Repeated fingerprints are allowed: those entries cover the same source lines,
though they may assert different behavior. A repeated fingerprint adds no new
line coverage by itself.

The runner prints a concise case and coverage summary, including uncovered line
ranges. It always writes the full report to
`.open-contract/coverage/latest.json`; `--coverage-report <path>` selects a
different location. A run exits nonzero when cases fail, a required fingerprint
is missing or mismatched, or the manifest's coverage threshold is not met.

Run the runner, bootstrap-contract coverage checks, and language support checks
with:

```sh
make check-contracts
```

Cargo artifacts and Rust coverage reports are written under the user's home
cache directories rather than into the checkout.
