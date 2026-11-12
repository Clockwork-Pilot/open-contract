# Rust core

The crate keeps four responsibilities separate:

1. `manifest` validates a versioned package manifest and produces a backend-neutral plan.
2. `isolation` stages only declared paths under a fresh temporary root and invokes the selected backend.
3. `batch` loads JSON or YAML datasets, traverses cases and ordered steps, sends the root and nested package batches to one coordinator inside isolation, and retains raw outputs, errors, and coverage reports.
4. `verify` compares outputs with exact expected values or evaluates predicates after the adapter batch has finished. It also checks that native coverage reports include every declared validator and support source file.

Nested manifests listed by a parent are staged under the same randomized root. The Rust core starts one isolation session for the complete tree; the coordinator launches each language adapter inside that session, so nested execution inherits the parent's boundary. Their raw native coverage reports are collected with the parent's report; adapters can merge coverage formats that are language-specific.

The batch adapter protocol is one JSON document on stdin:

```json
{"contracts":[{"contract":"example","package":"","validator":"src/validator.py","requests":[{"key":{"target_id":"default","case_id":"case-1","step_id":null},"kind":"custom","config":{},"input":{"value":1}}]}]}
```

The coordinator returns one JSON document on stdout with a `contracts` array keyed by contract name. Each batch has `results` keyed by target, case, and optional step IDs. Each result has either `output` or `error`; `coverage` carries native coverage data and a `sources` array of exercised root-relative source paths. Stdout is reserved for this response. The adapter does not compare expectations or evaluate predicates.

The built-in runtime bridge selects Python or JavaScript from the validator file suffix. It starts a coordinator inside one isolation session; the coordinator launches the language batch adapters as children inside that same boundary. Run it with runtime-selected isolation:

```sh
cargo run --manifest-path contract-runner/rust/Cargo.toml -- run-manifest contract.yaml \
  --isolation landlock
```

For a custom batch adapter, pass `--runtime-profile` and `--adapter <command> [args...]`. `none`, `bwrap`, and `landlock` are runtime choices. Contract paths remain package-relative, and each run uses a fresh staged root.
