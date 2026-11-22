# Manifest bootstrap contract

This package's dataset drives the public manifest parser and planner in
`src/lib.rs`. The `bin/dataset-contract.rs` command-line entrypoint delegates to
`tests/support/validator.rs`, which passes each dataset case's actual manifest
or path input to that API. The coverage check runs this binary with the full
dataset; the validator captures each case's profile independently and restores
the aggregate profile after the batch.

The optional top-level `metadata` mapping is consumer-owned data. The Rust
manifest model preserves its JSON-compatible values for callers through
`ManifestPlan.manifest.metadata`; the runner does not interpret metadata or use
it when planning or executing contracts. Unknown top-level manifest fields
remain errors.

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

Dataset cases provide package paths or manifest YAML directly. The validator
materializes the package files needed by the manifest input. Cases requiring
an extra external condition such as a symlink escape declare `setup: true`; the
validator maps that case ID to setup, then passes the supplied manifest input
to the public parser and checks its actual result. Setup never replaces the
public call. Results may use `expected`, a sibling result `predicate`, or both.
The package check requires 100% function, instantiation, line, and region
coverage, plus matching case fingerprints. Generated LLVM profiles and reports
stay under the Open Contract root `.coverage/bootstrap-contracts/` directory.

Run only this contract and its required dataset coverage checker from the Open
Contract repository root:

```sh
make check-manifest-contract
```
