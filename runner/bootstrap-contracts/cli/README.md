# Rust runner CLI bootstrap contract

This bootstrap package owns the command-line interface used by
`runner/rust`. The Rust runner binary delegates its arguments to the public
`runner_cli::execute` entry point; the dataset invokes that same function.

The dataset shows complete CLI invocations. Each case gives the argument list,
any input files the CLI reads, and the complete exit code, stdout, and stderr
result. Cases with `setup: true` materialize those supplied files in a fresh
working directory before calling the CLI. The validator does not map fixture
IDs to hidden file contents or substitute a test-only operation for the CLI
call.

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

Generate the checked HTML coverage view with:

```sh
make check-cli-contract-html
```

The check runs the full dataset and requires 100% function, instantiation, line,
and LLVM region coverage plus matching 32-character fingerprints. Coverage
artifacts are written under the root `.coverage/bootstrap-contracts/` directory.

Every manifest loaded for execution by `run-manifest`, including nested
manifests, must have a filename whose stem contains an exact dot-delimited
`open-contract` segment and ends in `.yaml` or `.yml`. When
`--no-nested-contracts` is used, only the root manifest is loaded for execution.
`show-isolation-plan` remains a read-only inspection command and accepts any
root manifest filename.
