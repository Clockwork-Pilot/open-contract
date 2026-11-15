# Manifest bootstrap contract

This package's dataset drives the public manifest parser and planner in
`src/lib.rs`. The dataset test entrypoint is `tests/support/validator.rs`,
which passes each dataset case's actual manifest or path input to that API.
`src/main.rs` is the command-line entrypoint and delegates to the same
validator for normal execution.

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

Dataset cases provide package paths or manifest YAML directly. The test
validator materializes package files from each manifest input. Cases requiring
an external condition such as a symlink escape declare `setup: true`; the
validator maps that case ID to setup, then passes the supplied manifest input
to the public parser and checks its result. Setup never replaces the public
call. The package check requires 100% dataset line and region coverage and
matching case fingerprints. Generated LLVM profiles and reports stay under the
Open Contract root `.coverage/bootstrap-contracts/` directory.

Run only this contract and its required dataset coverage checker from the Open
Contract repository root:

```sh
make check-manifest-contract
```
