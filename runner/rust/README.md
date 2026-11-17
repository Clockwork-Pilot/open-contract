# Contract runner

The runner validates a contract manifest, loads its JSON or YAML dataset, and
executes the root contract and any declared nested contracts in one run. All
contracts in the tree use the selected isolation mode. In `tempdir` mode they
share a temporary staged workspace; in other modes they execute from their
source locations under the selected filesystem policy. Results remain grouped
by contract, while coverage is combined in the root report.

The runner invokes each contract's declared `execution.command` once with the
complete flattened batch for that contract. The command receives one
contract-local JSON document on stdin, with case and step identities,
optional setup IDs, and no expected outputs or predicates:

```json
{"contract":"example","package":".","validator":"src/validator.py","coverage_sources":["src/validator.py"],"requests":[{"key":{"case_id":"case-1","step_id":null},"input":{"value":1},"setup":null,"parallel":false}]}
```

The contract-owned command imports its language support package and validator,
then returns one JSON document on stdout. Its `results` preserve request keys
and contain either `output` or `error`; `coverage` carries the native report
and exercised source paths. The support package is an importable interface, not
a second CLI or runner. The runner handles dataset and step traversal,
expected-output matching, predicate evaluation, coverage enforcement, and
reporting. Adjacent
steps marked `parallel: true` are grouped into one concurrent adapter request;
other steps run sequentially.

## CLI capabilities and configuration

The executable supports two commands:

```sh
contract-runner run-manifest <contract.yaml> [options]
contract-runner show-isolation-plan <contract.yaml> [options]
```

`run-manifest` executes the dataset and writes its coverage report. It accepts
all options in the table below. `show-isolation-plan` only resolves and prints
the isolation plan; it accepts `--isolation` and `--isolation-runtime`, but not
coverage or fingerprint options.

| Option | Command | Purpose |
| --- | --- | --- |
| `--isolation <auto\|tempdir\|none\|bwrap\|landlock>` | Both | Select the execution boundary. Defaults to `OPEN_CONTRACT_ISOLATION`, then `auto` if unset. |
| `--isolation-runtime <path>` | Both | Replace the bundled runtime YAML for `bwrap` or `landlock`. Invalid with `none` and `tempdir`. |
| `--coverage-report <path>` | `run-manifest` | Write the coverage report. Defaults to `.open-contract/coverage/latest.json`. |
| `--disable-fingerprints` | `run-manifest` | Skip per-case fingerprint checks while retaining output and aggregate coverage checks. Equivalent to `OPEN_CONTRACT_DISABLE_FINGERPRINTS=1`. |
| `--update-fingerprints` | `run-manifest` | Rewrite fingerprints after all cases and coverage requirements pass. Cannot be combined with `--disable-fingerprints` or `OPEN_CONTRACT_DISABLE_FINGERPRINTS=1`. |

The runner reads these environment variables from its own process:

| Variable | Used by | Meaning |
| --- | --- | --- |
| `OPEN_CONTRACT_ISOLATION` | Both commands | Default isolation choice: `auto`, `tempdir`, `none`, `bwrap`, or `landlock`. An explicit `--isolation` takes precedence. |
| `OPEN_CONTRACT_DISABLE_FINGERPRINTS` | `run-manifest` | Disable per-case fingerprints when set to `1`, `true`, `yes`, or `on`; `0`, `false`, `no`, or `off` leaves them enabled. Other values are errors. |
| `OPEN_CONTRACT_LANGUAGE_PACKAGES` | Both commands when resolving a manifest's `language_package` | Optional JSON object mapping logical package IDs to package directories, for example `{"python":"/opt/contracts/python"}`. Without a mapping, the runner resolves packages under `contract-languages/`. |

The runner does not read `OPEN_CONTRACT_LANGUAGE_PATH` from the caller's shell.
For an isolated adapter command, it sets that variable from the selected
language runtime's `environment_paths` entry, which points to the resolved
language support package. Other adapter environment values come from the
selected isolation runtime YAML. These runtime-provided values are distinct
from the runner's CLI configuration above.

The manifest declares the contract command and, when needed, one logical
`language_package` identifier. Runner configuration resolves that identifier
and provides the support package location through `OPEN_CONTRACT_LANGUAGE_PATH`.
The manifest contains no host paths or language-specific dependency file lists.

For example:

```yaml
execution:
  command: [python3, contract_command.py]
language_package: python
```

Run the Rust executor with runtime-selected isolation:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- run-manifest contract.yaml \
  --isolation landlock \
  --coverage-report /tmp/contract-coverage.json
```

`OPEN_CONTRACT_ISOLATION` supplies the isolation mode when `--isolation` is
omitted. An explicit command-line value takes precedence. This also applies to
`show-isolation-plan`:

```sh
OPEN_CONTRACT_ISOLATION=landlock cargo run --manifest-path runner/rust/Cargo.toml -- \
  run-manifest contract.yaml
```

`tempdir`, `none`, `bwrap`, and `landlock` are runtime execution choices.
`tempdir` preserves the copied-workspace behavior: it copies declared contract
files to a fresh temporary tree and accepts package-relative read/write paths.
`none`, `bwrap`, and `landlock` execute from the checkout without copying
contract files. In those modes, `filesystem.read` and `filesystem.write` paths
may be relative to the contract directory (including paths leaving it) or
absolute host paths. `none` applies no filesystem policy; bubblewrap and
Landlock enforce the declared paths using their native mechanisms.

Nested contracts in a `tempdir` run share one randomized temporary tree. In
the direct modes, nested contracts remain at their source paths in the
checkout. Rust invokes each contract's declared command once for its full
batch. The logical `language_package` identifier is resolved by runner
configuration; it does not contain a host path or a list of support files.

For `bwrap` and `landlock`, the runner selects the corresponding bundled system
runtime YAML from `runner/isolation-runtime/<backend>.yaml`. Override that
selection with `--isolation-runtime <path>`; the override replaces the bundled
file. Each YAML lists absolute host paths made available read-only and
environment values passed to the isolated command. Paths must exist. For
example:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- run-manifest contract.yaml \
  --isolation bwrap \
  --isolation-runtime ./runtime/bwrap.yaml
```

Bubblewrap mounts listed paths read-only. Landlock grants read and execute
access to listed paths. The `command_executable` setting explicitly grants
access to the executable resolved from `execution.command`, including an
executable supplied through an environment reference such as `${NODE_BIN}`.
`--isolation-runtime` is only accepted with `bwrap` and `landlock`; it is not
accepted with `none` or `tempdir`. Runtime YAML describes host runtime
dependencies such as system libraries and executables. Contract-owned or
shared contract files belong in `filesystem.read`/`filesystem.write`.

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

Per-entry fingerprints can be disabled for a diagnostic run with
`--disable-fingerprints` or `OPEN_CONTRACT_DISABLE_FINGERPRINTS=1`. The runner
passes this choice to the language adapter, which skips per-entry coverage
scopes. The runner then skips fingerprint presence, format, and match checks;
aggregate dataset line coverage and expected-output checks remain active. No
dataset fields need to be changed. The environment value accepts `1`, `true`,
`yes`, or `on` to disable, and `0`, `false`, `no`, or `off` to leave checks on.

Adapters return the normalized per-case line data with the batch; the runner
does not parse language-specific native report formats. The artifact is written
even when case or coverage checks fail.

The shared `runner-errors` package preserves the CLI exit code and adds
runner context to errors returned by contract execution. Its bootstrap contract
exercises rendering and propagation from its dataset.
