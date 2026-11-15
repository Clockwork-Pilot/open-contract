# Filesystem isolation bootstrap contract

This package's dataset exercises the Rust runner's filesystem staging and
isolation behavior. The implementation is the library in `src/lib.rs` and
`src/backend.rs`; it builds and works independently of the test suite.
Its cases check declared files, writes, nested package files, and fresh staging
roots. Command cases cover both Landlock and Bubblewrap, including Bubblewrap
command-resolution errors, nonzero command results, and a successful command
that must return the expected stdout. Dataset coverage comes from running those
cases.

The integration-test validator in `tests/support/validator.rs` reads dataset
entries and calls the public isolation API. Harness setup is mapped by dataset
case ID in `tests/support/dataset_setup.rs`, outside the public input. A case
that uses setup declares `setup: true` beside its `id`. Setup may prepare
external conditions before the public call: `bubblewrap-unavailable` hides
`bwrap` from `PATH`, and the successful command case supplies a test launcher
because this environment does not permit Bubblewrap user namespaces. The case
still sends its actual dataset command through the public isolation API and
checks the returned result. Setup code must not replace that call, fabricate
its result, or call backend implementation helpers to add lines to the case's
dataset fingerprint; keep such helper checks in the separate unit-test path.

`isolation-package.fixture.yaml` is input data for this staging test harness.
It is parsed by `runner-manifest` to describe the package tree being staged; it
is not an Open Contract manifest and does not configure isolation for this
contract's own execution.

The coverage check runs only the `dataset` integration-test target and requires
100% coverage of implementation files under `src`. Validator, fixture, and
unit-test code do not contribute to that percentage. Use unit tests for internal
syscall failures that cannot be reached through the public input.

Isolation belongs to the Rust runner. A contract-language implementation must
not establish or weaken the boundary; language support is separate under
`contract-languages/`. The contract manifest describes package files, while the
runner applies the selected isolation backend.

Run only this contract and its required dataset coverage checker from the
Open Contract repository root:

```sh
make check-isolation-contract
```
