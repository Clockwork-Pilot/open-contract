# Rust core

The crate keeps four responsibilities separate:

1. `manifest` validates a versioned package manifest and produces a backend-neutral plan.
2. `isolation` stages only declared paths under a fresh temporary root and invokes the selected backend.
3. `batch` loads JSON or YAML datasets, traverses cases and ordered steps, and sends only flattened validator input requests to the language adapter inside isolation. It retains raw outputs, errors, and coverage reports for root and nested contracts.
4. `verify` compares outputs with exact expected values or evaluates predicates after execution finishes. It also checks that native coverage reports include every declared validator and support source file.

Nested manifests listed by a parent are staged under the same randomized root. The Rust core starts one isolation session for the complete tree and invokes each listed contract's cases in that session. Child execution therefore inherits the parent's boundary. Child and parent coverage reports are combined in the root report, while their case results remain separate.

The adapter protocol is one JSON document on stdin. Rust provides each contract's flattened case and step requests, with no expected outputs or predicates:

```json
{"contracts":[{"contract":"example","package":"","validator":"src/validator.py","requests":[{"key":{"target_id":"default","case_id":"case-1","step_id":null},"kind":"custom","config":{},"input":{"value":1}}]}]}
```

The language adapter returns one JSON document on stdout with a `contracts` array. Its `results` fields have request keys and either `output` or `error`; `coverage` carries the native report and exercised source paths. Stdout is reserved for this response. The adapter does not read datasets, define steps, compare expectations, or evaluate predicates.

The root manifest declares the execution command and optional runtime profile.
Rust traverses the dataset and sends its flattened input requests to that
command in one batch. Rust itself owns dataset semantics and output verification.

For example:

```yaml
execution:
  command: [python3, -m, open_contracts_runner.tree_batch_adapter]
  runtime_profile: all
```

Run the Rust executor with runtime-selected isolation:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- run-manifest contract.yaml \
  --isolation landlock \
  --coverage-report /tmp/contract-coverage.json
```

`none`, `bwrap`, and `landlock` are runtime isolation choices. Contract paths
remain package-relative, and each run uses a fresh staged root.

`--coverage-report <path>` is optional. It writes each executed contract's raw
native report, declared and covered source paths, and missing source paths to a
separate JSON file. It also lists executable, covered, and uncovered line
numbers per source file, plus covered lines per dataset case and step. Language
adapters translate their native coverage into this shared line format; Rust
does not parse language-specific coverage reports. Only dataset execution
contributes to contract coverage.
When enabled, the runner also prints a compact per-file line summary to stderr,
including uncovered line ranges, so the gaps are visible without opening the
JSON artifact. Dataset coverage is always reported independently. A manifest may
explicitly list rare supplemental test files; adapters run them only after the
dataset reaches 90% line coverage and report their results and line coverage in a
separate section. Ordinary language test suites are never included. Supplemental
coverage does not change dataset fingerprints or dataset coverage percentages.
Each declared file must also appear in `filesystem.read`; Python modules and
JavaScript modules export a `run()` function, which may throw or return `false`
to report failure. For example:

```yaml
files:
  supplemental_tests: [tests/edge_cases.py]
filesystem:
  read: [dataset.yaml, validator.py, tests/edge_cases.py]
```

Adapters return the normalized per-case line data with the batch; the Rust core
does not parse language-specific native report formats. The artifact is written
even when case or coverage checks fail.

The shared `open-contract-errors` package preserves the CLI exit code and adds
runner context to errors returned by contract execution. Its bootstrap contract
exercises rendering and propagation from its dataset.
