# Batch execution bootstrap contract

This bootstrap contract exercises `src/lib.rs` directly with public batch
inputs in `dataset.json`. Its Rust dataset driver creates temporary manifests
and supplies ordinary adapter results so the cases can check output matching,
predicates, sequences, dataset validation, and coverage fingerprints.

Harness-only scenarios are Rust unit tests under `tests/support/`; their adapter
behavior and manifest setup are test configuration, separate from batch input
in `dataset.json`. The unit test in `src/lib.rs` also loads the public dataset
entries and reuses them as parsing and request-flattening inputs.

The coverage check reports dataset coverage, unit-test-only coverage, and their
combined coverage against the same Rust implementation-line total. It requires
100% combined coverage and requires the dataset to contribute more covered
implementation lines than unit tests.

For consumer contracts, the Rust runner reads the manifest, starts the selected
isolation boundary, executes its declared language-adapter command once for the
contract tree, and verifies returned outputs afterward. Language adapters only
execute validators and report results and coverage; they do not read datasets,
iterate cases or steps, evaluate predicates, or provide isolation. That runtime
path is exercised when running consumer contracts. The batch bootstrap
contract focuses on the runner's public batch behavior, while protocol edge
cases are covered by its unit tests.

Datasets use format version 1 and may be JSON or YAML. Prefer YAML for
hand-authored datasets. Cases and steps can specify exact expected output,
predicates, or an expected validator error. A sequence can also check its
aggregate output. Each case and step that pins `coverage_fingerprint` uses the
first 128 bits of SHA-256 over sorted package-relative covered files and lines.
Every case and step must declare a nonzero `coverage_fingerprint`; missing,
malformed, zero, and mismatched values fail. Repeated fingerprints are allowed:
they mean those inputs covered the same source lines, while still permitting
distinct behavior checks on those inputs. They do not increase the union of
covered source lines, so use the combined dataset coverage report to see whether
new cases add line coverage.

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
