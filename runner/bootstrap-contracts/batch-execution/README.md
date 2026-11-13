# Batch execution bootstrap contract

This bootstrap contract exercises `src/lib.rs` directly with the cases in
`dataset.json`. Its Rust dataset driver creates small temporary manifests and
feeds their datasets and adapter responses through the batch implementation.
The cases include successful batches, sequences, nested execution, malformed
datasets, adapter protocol failures, coverage validation, and expected errors.

The fixtures are dataset cases for the batch API; Cargo unit tests are not run
or counted in its coverage. The coverage command measures only `src/lib.rs`
while all declared dataset cases execute.

For consumer contracts, the Rust runner reads the manifest, starts the selected
isolation boundary, executes its declared language-adapter command once for the
contract tree, and verifies returned outputs afterward. Language adapters only
execute validators and report results and coverage; they do not read datasets,
iterate cases or steps, evaluate predicates, or provide isolation. That runtime
path is exercised when running consumer contracts, and is tested here through
`BatchAdapter` fixture responses as needed for protocol edge cases.

Datasets use format version 1 and may be JSON or YAML. Prefer YAML for
hand-authored datasets. Cases and steps can specify exact expected output,
predicates, or an expected validator error. A sequence can also check its
aggregate output. Each case and step that pins `coverage_fingerprint` uses the
first 128 bits of SHA-256 over sorted package-relative covered files and lines.

Run the dataset and its implementation coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/batch-execution
```
