# Manifest bootstrap contract

This package's dataset exercises the Rust manifest parser and planner in
`src/lib.rs`. Its dataset executable is `src/main.rs`; coverage
for this package comes from running that dataset executable, not from unit
tests.

The Rust runner owns manifest loading and planning. The `execution.command`
field names the language adapter command invoked once for the contract tree.
Rust supplies the flattened case and step inputs; the adapter returns validator
outputs without deciding dataset structure or verifying expected results. Language-specific validator interfaces and runtimes live
separately under `contract-languages/`.

Run the dataset coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/manifest
```
