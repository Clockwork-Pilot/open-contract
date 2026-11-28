# Rust runner CLI bootstrap contract

This bootstrap package owns the command-line interface used by
`runner/rust`. The Rust runner binary delegates its arguments to the public
`runner_cli::execute` entry point; the dataset invokes that same function.

The dataset shows complete CLI invocations: each case gives its real argument
input and complete exit code, stdout, and stderr result. A setup fixture ID
resolves external files and environment values in the test fixture registry;
the validator then passes the case's input unchanged to the CLI.

`run-manifest` prints a `Contracts executed` section before the aggregate
dataset summary. The section lists each contract in recursive traversal order
with that contract's passed and total dataset-case counts. The aggregate line
and any per-case failure details follow.

Nested manifests run by default. Pass `--no-nested-contracts` to execute only
the root manifest; nested contracts are omitted from the execution and coverage
reports.

Run the checks from the `open-contract` repository root:

```sh
make check-cli-contract
```

Generate an offline HTML coverage report from the language adapters' coverage
entities with:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- run-manifest \
  runner/bootstrap-contracts/cli/open-contract.yaml \
  --coverage-html .coverage/cli-html
```

The report contains an index for contracts and their declared datasets, then
source pages with line tables and per-entity locations, status, and adapter
reported counts. Dataset links point to their contract's coverage union because
the adapter executes all datasets for a contract in one batch. Metric details
that an adapter does not provide are labeled unavailable. This command does not
invoke LLVM to generate HTML; it renders the entity data returned by the
JavaScript, Python, or Rust adapter.

Every manifest loaded for execution by `run-manifest`, including nested
manifests, must have a filename whose stem contains an exact dot-delimited
`open-contract` segment and ends in `.yaml` or `.yml`. When
`--no-nested-contracts` is used, only the root manifest is loaded for execution.
`show-isolation-plan` remains a read-only inspection command and accepts any
root manifest filename.
