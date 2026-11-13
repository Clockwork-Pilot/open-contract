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

Command arguments may reference explicitly supplied environment variables using
`${NAME}` syntax. Each reference is expanded as part of one argument, without
shell parsing or word splitting. For example, `"${NODE_BIN}"` lets the caller
select the executable path while the contract names the variable it requires.
Unset variables fail before command execution.

Run the dataset coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/manifest
```
