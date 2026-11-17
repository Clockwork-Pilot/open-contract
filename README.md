# Open Contract

Open Contract has one generic runner, written in Rust. It plans a manifest,
starts one filesystem isolation boundary, runs the dataset batch and declared
nested contracts, then verifies outputs and predicates after execution.

## Repository layout

```text
runner/rust/                         Generic runner and application composition
runner/tools/                        Developer tooling for bootstrap dataset coverage
contract-languages/python/           Python validator adapter and support
contract-languages/javascript/        JavaScript validator adapter and support
runner/bootstrap-contracts/          Open Contract's own contract packages
  manifest/                          Manifest planning contract and Rust code
  isolation/                         Filesystem isolation contract and Rust code
  batch-execution/                    Dataset, step, and nested execution contract
  verification/                      Expected output and predicate matching contract
  predicates/                        Predicate semantics and conformance corpus
bin/                                  Low-level isolation launch helpers
```

The Python and JavaScript directories provide language-specific validator
execution and native coverage collection. They do not select isolation, plan
contracts, traverse datasets, match expectations, or evaluate predicates.

Each Rust bootstrap package is stored next to the contract dataset that
exercises it. The Rust application depends on those packages; the bootstrap
contracts are not placeholder copies of code hidden in the application.

## Why contracts

A consumer dataset case describes one call to the contract's public
interface. Its `input` is passed unchanged to the public validator; sibling
`expected` and optional `predicate` describe the result the validator actually
returns. The language adapter invokes the validator with that input and
returns its result. The Rust runner checks the result against `expected` and
then evaluates the optional predicate. A returned error is part of that
observable result and is checked in the same way.

Dataset cases do not define fixtures, harness operations, or alternate
execution layers. When a test harness needs to prepare an external condition,
keep setup in validator-side code selected by the case ID and run it before
the public call. Then make the same public call with the dataset's actual
input and check the result it produces. Setup must not fabricate the result
or bypass the public interface. Open Contract runs each contract inside the
filesystem boundary declared by its manifest.

Dataset cases are the primary behavior checks because they describe what a
contract user can provide and observe. Use ordinary unit tests for internal
conditions the public interface cannot expose; those tests do not add dataset
coverage. Unit tests may reuse dataset inputs as regression examples.

## Run contracts

Select the isolation backend when invoking the Rust runner:

```sh
cargo run --manifest-path runner/rust/Cargo.toml -- \
  run-manifest runner/bootstrap-contracts/batch-execution/contract.yaml \
  --isolation landlock
```

The manifest's `files` entries (dataset, validator, support files, and nested
contracts) stay within the contract package. `filesystem.read` and
`filesystem.write` entries are host paths: relative entries resolve from the
contract directory, and absolute entries are accepted. They may refer to
shared files outside the contract directory.

`--isolation tempdir` preserves the original staged-copy behavior: declared
contract files are copied into a fresh temporary package tree before execution.
It accepts package-relative filesystem entries only. `none`, `landlock`, and
`bwrap` use the checkout directly. `none` applies no filesystem restrictions;
Landlock and bubblewrap apply the manifest's read/write paths to their native
filesystem views. Contract code should use relative paths for files inside its
package; external relative paths resolve from the source contract directory.

The root contract starts isolation once. Its dataset and declared nested
contracts execute within that boundary. Running a nested contract directly
starts a separate boundary. Nested coverage is combined into the root coverage
report while each contract's case results remain distinct.

## Isolation backends

- **Bubblewrap** provides a restricted filesystem view and namespace
  isolation. The runner mounts declared host paths and required runtime
  resources.
- **Landlock** restricts filesystem access to allowed paths. Paths remain
  at their host locations, and Landlock does not restrict networking.

Isolation startup failures are reported; the runner does not continue the
contract outside the requested boundary. `none` is available for trusted
checks and debugging only. `tempdir` provides a disposable copied workspace,
not an operating-system access boundary.

## Verification and coverage

Runner JSON and YAML datasets use the shared
[`dataset.v1.schema.json`](schemas/dataset.v1.schema.json) schema. The runner
validates each dataset against this root-level Open Contracts schema before
loading it; runner-specific checks then enforce unique IDs, valid setup
fixtures, and observed coverage fingerprints.
See [`schemas/README.md`](schemas/README.md) for how the dataset schema
differs from the bundle schema.

Language adapters receive batches of inputs and return serialized outputs,
errors, and native coverage. The Rust verifier compares outputs with exact
expected values or evaluates the shared predicate language after the batch
returns. Contracts are not assumed to be network services.

Bootstrap Rust packages run their dataset executable with
`runner/tools/check_bootstrap_contract_coverage.py`. The dataset must cover
100% of implementation lines and LLVM regions. Unit-test coverage is excluded
from this requirement; use unit tests for implementation details the public
dataset interface cannot exercise.

After running the contract checks, generate HTML reports for all bootstrap
contracts with:

```sh
make bootstrap-contract-coverage-html
```

Reports are written to `.coverage/bootstrap-contracts/<contract-name>/html/` at
the Open Contract repository root. The directory is ignored by Git. Each report
excludes sibling bootstrap crates used as dependencies; the shared
`dataset-harness` crate has no separate report.

Every Open Contract dataset case and step must declare a matching,
nonzero `coverage_fingerprint`. It hashes each covered package-relative source
path and the exact text of its covered lines in source order, without line
numbers. Inserting unrelated lines above covered code does not invalidate the
pin; changing covered code does. Missing, malformed, zero, or stale values
fail the case and must be refreshed when its observed coverage changes.
Coverage checks use the repository-pinned Rust toolchain because LLVM versions
can map source coverage to lines differently. Rustup installs the configured
toolchain and `llvm-tools-preview` component when they are missing.

For runner-executed datasets, run
`cargo run --manifest-path runner/rust/Cargo.toml -- run-manifest <contract.yaml> --update-fingerprints`
to write observed fingerprints back to the root and nested dataset files.
The runner prints each changed case or step and its old/new fingerprints,
along with the covered files and lines. It writes updates only when all dataset
outputs pass and coverage requirements are met. Review that scope before using
this option. Normal runs remain read only and fail on stale pins; the update
option cannot be combined with `--disable-fingerprints`.

Repeated fingerprints are allowed: those entries cover the same source text,
though they may assert different behavior. Fingerprints therefore provide a
dataset coverage duplication metric: repeated values identify cases that
reveal the same covered paths and lines. Treat repeats as a signal to review
whether cases are redundant and could be deduplicated; matching fingerprints
alone do not show that their inputs or assertions are interchangeable. A
repeated fingerprint adds no new line coverage by itself.

The runner prints a concise case and coverage summary, including uncovered line
ranges. It always writes the full report to
`.open-contract/coverage/latest.json`; `--coverage-report <path>` selects a
different location. A run exits nonzero when cases fail, a required fingerprint
is missing or mismatched, or the manifest's coverage threshold is not met.

Run the runner, bootstrap-contract coverage checks, and language support checks
with:

```sh
make check-contracts
```

Choose the isolation mode for every Open Contract example run by this goal with
`OPEN_CONTRACT_ISOLATION`. `auto` tries Bubblewrap, then Landlock, then a
temporary copied package, and finally unrestricted `none`. It reports the
selected mode and why stronger modes were skipped. The fallback checks whether
the backend can start a minimal command; failures from the actual contract do
not trigger a weaker retry. Explicit values `tempdir`, `none`, `landlock`, and
`bwrap` continue to select one mode without fallback:

```sh
OPEN_CONTRACT_ISOLATION=landlock make check-contracts
```

This setting applies to the Python, JavaScript, and nested Rust examples that
invoke the Rust runner. Bootstrap component checks call their Rust APIs
directly, so they do not use runner isolation. If unset, the Makefile uses
`auto`.

Each `run-manifest` invocation prints the selected backend before its dataset
summary. Explicit selections print `Isolation mode: <backend>`; automatic
selection prints `Isolation auto selected: <backend>` followed by fallback
details when applicable.

For a diagnostic runner invocation, set
`OPEN_CONTRACT_DISABLE_FINGERPRINTS=1` or pass `--disable-fingerprints` to the
Rust runner. This skips per-entry fingerprint collection and matching while
retaining expected-output checks and aggregate dataset coverage requirements.
Fingerprint checks remain enabled by default.

Cargo artifacts and Rust coverage reports are written under the user's home
cache directories rather than into the checkout.

## Troubleshooting

### Where runtime files are declared

Access requirements belong in versioned YAML at the layer that owns them:

| Resource | Declaration |
| --- | --- |
| Shared OS resources, such as Landlock access to `/dev/null` | `runner/isolation-runtime/<backend>.yaml` |
| Language adapter tools, libraries, and installed dependencies | `contract-languages/<language>/runtime.yaml` |
| Contract inputs, implementation, executable, and writable outputs | The contract manifest's file and filesystem declarations |

The runner combines the selected backend runtime with the declared language
runtime. `--isolation-runtime <file>` replaces the backend default; it does not
extend it. Keep intentional overrides in maintained YAML files. Do not generate
permission manifests or add hidden access grants in example scripts or Make
recipes.

Language packages own dependency installation. For example,
`make -C contract-languages/rust install-dependencies` installs the active Rust
toolchain's LLVM coverage tools and libraries into its ignored `.deps/` directory.
The Rust `runtime.yaml` declares those files and sets `LLVM_PROFDATA` and
`LLVM_COV` through `environment_paths`. The example invokes this installation
goal and uses the default isolation runtime, without granting access to the
host Rust sysroot. Python similarly declares its installed dependencies in its
language runtime. Tool locations in `environment_paths` must be included in
the language runtime's declared files.

### Permission denied (`EACCES`) under isolation

For a one-time diagnosis on Linux, use `strace` to record filesystem calls and
child-process startup. Install it with your system package manager if needed.
Run from the Open Contract root, selecting the backend that failed:

```sh
mkdir -p .coverage/debug
strace -f -yy -s 256 -e trace=%file,%process \
  -o .coverage/debug/rust-nested.strace \
  make check-rust-nested-contracts OPEN_CONTRACT_ISOLATION=landlock
grep -n -E 'EACCES|EPERM' .coverage/debug/rust-nested.strace
```

Replace the Make goal with the failing check. `-f` follows child processes;
`-yy` annotates file descriptors with paths. Inspect the denied syscall's path,
open flags, and nearby process calls to identify the required access. See the
[strace manual](https://man7.org/linux/man-pages/man1/strace.1.html) for options.

For example, the Rust adapter previously failed while starting `llvm-profdata`.
The trace showed `openat(..., "/dev/null", O_RDONLY...) = -1 EACCES`: opening
the child's stdin failed before the executable started. Read access to
`/dev/null` is now declared in `runner/isolation-runtime/landlock.yaml`.
Bubblewrap supplies its device directory with `--dev /dev`.

Declare contract-owned data access in the contract manifest and runtime/tool
access in the appropriate runtime manifest. Check ordinary filesystem permissions
as well: `EACCES` alone does not establish that Landlock caused the denial.
Rerun the same check without tracing after the correction. `strace` is an
optional debugging tool, not a runner dependency or part of normal execution.
