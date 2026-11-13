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
  manifest/rust/                     Manifest planning implementation
  isolation/rust/                    Filesystem isolation implementation
  batch-execution/rust/               Dataset, step, and nested execution
  verification/rust/                 Expected output and predicate matching
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

A contract makes the behavior to exercise explicit in its manifest and
dataset. Open Contract runs each contract inside a filesystem boundary that
limits it to its declared data and source, plus the runtime resources it needs.
That makes the contract the boundary we can control: its dataset states what
must be exercised, and its isolation prevents the check from silently relying
on unrelated repository files.

Generic unit tests are not the coverage target. Tests that agents generate by
calling individual functions can cover convenient helper functions without
specifying which behavior matters to the contract. Bootstrap coverage therefore
comes from running each contract's dataset against its implementation. The goal
is 100% coverage of the implementation in scope, showing that the dataset
exercises all of its instrumented code paths. This is evidence about what the
contract exercises, not by itself a proof that the implementation is correct.

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

Bootstrap Rust packages are exercised by dataset executables. Coverage excludes
unit-test runs and is collected only while those datasets execute. Run the
runner, bootstrap-contract coverage checks, and language support checks with:

```sh
make check-contracts
```

Cargo artifacts and Rust coverage reports are written under the user's home
cache directories rather than into the checkout.
