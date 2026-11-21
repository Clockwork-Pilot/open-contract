# Nested Rust contracts

This example has a parent contract and a physically independent child contract.
Each owns its dataset, public validator, and Rust command. Both commands call
`open-contract-language-rust::run_dataset`; the runner executes each contract's
complete dataset batch and verifies the returned values and coverage.

From the `open-contract` directory, build each contract command with LLVM
coverage enabled, then run the parent tree:

```sh
make check-rust-nested-contracts
```

Set `OPEN_CONTRACT_ISOLATION=landlock` (or `bwrap`, `tempdir`, or `none`) to
select the runner mode. The root `make check-contracts` goal passes the same
setting to all runner-backed examples.

The datasets pin the observed 32-character fingerprints; code changes that
alter exercised lines will make the runner print the replacement values. The
run requires `llvm-profdata` and `llvm-cov` matching the compiler version.
Install Rust's LLVM tools once with `rustup component add llvm-tools-preview`;
the example invokes the Rust adapter's `install-dependencies` Make goal, which
copies the tools and LLVM libraries into the adapter's ignored `.deps/` directory.
The manifests explicitly grant read access to `.deps/` and set the tool paths in
the command environment. The runner uses the default backend runtime and does
not load Rust-adapter package metadata.

The runner writes the combined report to `.coverage/rust-nested-report.json`.

`src/main.rs` reads the runner's batch request from stdin and passes each whole
dataset entry to `src/validator.rs`. The validator extracts the real `input`
and invokes the public function in `src/implementation.rs`. Both are declared
as coverage sources; the command plumbing is outside that coverage scope.
Contract commands are compiled before execution; the runner only stages the
executable and files declared by each manifest.
