# Open Contract

Open Contract provides a generic runner and isolation for portable contracts.
Each contract package has a manifest listing its dataset, validator, and other
contract files. This repository may contain Open Contract's own contracts, but
not consumer-specific contracts.

## Goals

- Keep contract data versioned, portable, and language-neutral.
- Allow a contract to include or reference nested contracts.
- Let parent contracts declare nested-contract invocations as data; use manifests
  to identify contract files rather than shell commands.
- When a parent runs a nested contract, stage and execute it inside the parent's
  single isolation boundary. Running that nested contract independently starts
  its own isolation boundary.
- Keep runners consistent across supported languages.
- Open Contract provides isolation and language-adapter interfaces. Contract
  code implements its validators or evaluators in its chosen language.
- Keep execution and verification separate. Inside isolation, contract logic
  receives input and produces output or artifacts. Open Contract checks those
  outputs outside isolation with the Open Contract verifier. Predicate
  evaluation belongs to Open Contract, not to contract languages or individual
  validator runtimes. Do not assume contracts are network services or require
  a network-style protocol.
- Keep the Rust core in four layers: manifest planning, isolation, batch
  execution, and output verification. The root executor starts isolation once;
  its coordinator invokes each contract's declared command once with that
  contract's complete batch, including nested contracts, inside the same
  boundary. Verify outputs and evaluate predicates after the batch returns,
  outside isolation.
- Let a contract identify its language adapter with one explicit
  `language_adapter` path to the adapter's `runtime.yaml`. The runner loads only
  that file; it does not map language names to packages. Adapter runtime files
  and environment come from that manifest, not duplicated contract
  `filesystem.read` entries. Keep contract-owned resources in the contract's
  filesystem policy.
- A language support package is an importable interface used by the
  contract-owned command. It iterates the supplied batch and invokes the
  contract validator, returning raw outputs/errors and coverage data. It does
  not parse manifests, isolate processes, compare expected values, or evaluate
  predicates. Rust treats each command as opaque and performs verification
  after all batch commands finish.
- Isolate each contract to its own data and source, plus the runner resources it
  needs. Keep other access explicit.
- Keep manifests isolation-backend agnostic: describe logical read and write
  paths, while the runner selects the backend at runtime and maps those paths to
  fresh per-check locations.
- Contract file paths are package-relative. Each execution stages or mounts the
  contract at a fresh randomized root, so contracts must not rely on stable
  absolute paths.
- Consumer Open Contract datasets have a version and one root `cases` list.
  Each case is either one call (`id`, `input`, `expected`, optional sibling
  `predicate`, optional `setup` fixture ID, and `coverage_fingerprint`) or an
  ordered `steps` list containing those call fields. `input` is the actual
  value passed to the public validator. `setup` is passed to the validator as
  context and must not replace or change that input. Dataset fields must not
  describe fixtures, harness operations, or alternate execution layers.
  Adjacent steps marked `parallel: true` form a concurrent group; other steps
  execute in order. There is no sequence-level input, expected result,
  predicate, or fingerprint. The runner checks each call's returned result
  against its sibling `expected` and optional `predicate`.
- Organize consumer datasets to help readers understand the contract's behavior:
  keep one dataset when it is clear enough, and split a larger set when focused
  groups such as normal use, meaningful boundaries, or failures provide a useful
  view. Use clear names and representative cases. Descriptions and semantic
  categories remain optional; do not add schema requirements to police whether a
  grouping is meaningful. Dataset grouping is for readers and maintenance, not
  selective execution: all declared datasets run together, their coverage is
  unioned before the shared manifest threshold is enforced, and case fingerprints
  remain independent.
- Keep the runner generic; consumer projects provide their own contracts and
  project-specific dependencies.
- Keep bootstrap predicate contracts as the dataset predicate semantics and
  conformance source, not as a runtime dependency of contract execution. Rust
  implementations may reuse their evaluator as a separately tested component.
- Treat the Rust core's prerequisite contracts as bootstrap contracts under
  `runner/bootstrap-contracts`: one each for manifest parsing and planning, filesystem
  isolation, batch dataset and step execution with coverage collection, and
  post-batch expected-output and predicate verification. Use these to exercise
  the corresponding Rust layers as the core develops.
- Treat dataset execution as the primary coverage source. Run the full contract
  dataset first and report its covered files and line numbers separately from
  any supplemental test coverage.
- Apply that dataset-first model to every Rust bootstrap package, including
  shared dependencies such as `dataset-harness` and `manifest`. Each package
  owns a JSON or YAML dataset that passes real inputs to its public API, and
  its Makefile check runs the package-scoped dataset coverage checker. Require
  100% dataset function, instantiation, line, and region coverage and an exact
  32-character lowercase hexadecimal fingerprint on every case. Unit-test
  coverage cannot make up for a dataset gap. Keep raw LLVM profiles and other
  generated coverage artifacts under the Open Contract root `.coverage/`,
  never inside a bootstrap package.
- For bootstrap datasets that support the `setup` marker, it is the only
  dataset-level harness control: `setup: true` and the case ID select setup
  that runs before the public call. Setup must not replace that call, invent a
  result, or call implementation internals to claim dataset coverage. Keep
  bootstrap setup controls outside `input`; do not add harness-only
  operations or placeholder inputs.
- When developing a contract toward 100% coverage, run its dataset with the
  coverage report, inspect uncovered files and lines, and map each gap to a
  behavior. If LLVM reports missing lines but its LCOV file has no line entries,
  inspect the zero-count region locations (line and column) to find uncovered
  expressions within partially covered lines. Group gaps by behavior and use
  package-scoped coverage while iterating; add meaningful dataset cases that
  exercise related paths and fix implementation defects those cases expose.
  Do not add cases solely to touch
  lines, and measure contract implementation files rather than dataset harness
  code. Once a contract reports a coverage failure, focus on that contract:
  inspect its uncovered files and lines, make the smallest meaningful dataset
  or implementation change, and rerun only that contract while iterating. Do
  not rerun contracts that already pass or the full suite until the focused
  coverage gate passes. Then confirm the repaired contract's report reaches
  100% for its implementation before moving on.
- Ordinary unit tests never contribute to dataset coverage or fingerprints. A
  contract may use them for internal helper behavior that the public interface
  cannot expose. Keep their setup and calls in the unit-test path; do not invoke
  unit-test helpers from a dataset validator to add implementation lines to a
  case fingerprint. Report unit-test coverage separately from dataset coverage.
  Every supplemental or mock test must document why a public dataset call
  cannot exercise it and identify the exact behavior or branches it checks.
- Rust runtime code must return actionable errors instead of panicking. Do not
  use `assert!`, `panic!`, `unwrap`, `expect`, `todo!`, `unimplemented!`, or
  `unreachable!` in production paths; use `Result`, `?`, and explicit error
  messages. Assertions and panic-prone helpers may appear in test-only code,
  which is never counted as contract coverage. Enforce this with the Rust
  runtime safety check, and include dataset cases for expected
  validator errors so the runner proves they are reported as results rather
  than crashing the contract run.
- Require adapters to report normalized per-file executable and covered lines
  for dataset execution, while preserving native reports as optional debug data.
  Keep explicit supplemental-test results and line coverage in a separate report.
- Pin observed dataset coverage with a required nonzero 32-character lowercase
  hexadecimal `coverage_fingerprint` on every direct call and every step. After
  a run, inspect the call's covered files, lines, observed fingerprint,
  expected fingerprint, and baseline match in the runner output or
  `--coverage-report`. A mismatch fails that call and reports both values.
  Review the scope's covered lines before updating a fingerprint; do not
  refresh fingerprints blindly. Parallel steps share their group's coverage
  scope and each still pins its own fingerprint.
- Treat fingerprints as a dataset coverage duplication metric: repeated
  fingerprints mean cases reveal the same covered paths and lines, so review
  whether those cases are redundant and can be deduplicated. A match alone
  does not prove the inputs or assertions are interchangeable; retain cases
  that cover distinct behavior.
- Implement predicate semantics in the Rust predicate contract and run its
  shared predicate dataset against that Rust implementation.
- Dogfood Open Contract when its Rust core can run and verify a repository
  contract through the same manifest, isolation, and verification path offered
  to consumers. Keep only the minimal bootstrap needed to reach that point.
- Collect nested-contract code coverage when the child runs as part of its
  parent. Combine parent and executed-child coverage in the root report, while
  reporting case results separately.

## Isolation

- Declare runtime access in versioned YAML at its owning layer. Shared OS
  resources belong in `runner/isolation-runtime/<backend>.yaml`; adapter tools,
  libraries, and dependencies belong in the adapter's `runtime.yaml`; contract
  data, executable files, and writable output paths belong in that contract's
  manifest filesystem policy.
- Language dependency installation may populate an ignored package-local
  `.deps/` directory. The adapter's `runtime.yaml` declares the required paths
  and environment. Do not expose an entire host toolchain to supply a few
  adapter tools.
- Do not generate runtime permission manifests in shell scripts or Make recipes,
  or hide additional access grants in command wrappers. Use the default backend
  manifest together with each contract's declared filesystem policy. An explicit `--isolation-runtime`
  override must point to a maintained, reviewable YAML configuration; it replaces
  the backend default, rather than extending it. Troubleshooting fixes must be
  applied to the owning runtime declaration and checked using normal invocation.

- **Bubblewrap** provides a restricted filesystem view and namespace isolation.
  Keep mounts limited to the contract and required runtime resources.
- **Landlock** limits filesystem access to allowed paths. It does not hide paths
  or restrict network access, so it is filesystem control rather than a complete
  sandbox.

Report isolation failures clearly and describe each mechanism's limits.
