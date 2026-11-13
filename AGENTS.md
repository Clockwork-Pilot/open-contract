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
- Open Contract provides isolation and supported language runtimes. Contract
  code implements its validators or evaluators in its chosen language.
- Keep execution and verification separate. Inside isolation, contract logic
  receives input and produces output or artifacts. Open Contract checks those
  outputs outside isolation with the Open Contract verifier. Predicate
  evaluation belongs to Open Contract, not to contract languages or individual
  validator runtimes. Do not assume contracts are network services or require
  a network-style protocol.
- Keep the Rust core in four layers: manifest planning, isolation, batch
  execution, and output verification. The root executor starts isolation once;
  its coordinator runs language adapters and listed nested contracts inside
  that same boundary. Verify outputs and evaluate predicates after the batch
  returns, outside isolation.
- Isolate each contract to its own data and source, plus the runner resources it
  needs. Keep other access explicit.
- Keep manifests isolation-backend agnostic: describe logical read and write
  paths, while the runner selects the backend at runtime and maps those paths to
  fresh per-check locations.
- Contract file paths are package-relative. Each execution stages or mounts the
  contract at a fresh randomized root, so contracts must not rely on stable
  absolute paths.
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
- Ordinary unit tests never contribute to contract coverage. A contract may
  explicitly list rare supplemental test files in its manifest for behavior
  that datasets cannot reach. Run dataset cases first; run those files only
  after dataset-only line coverage reaches 90%. Report their coverage separately
  and never let it change dataset coverage, fingerprints, or dataset coverage
  percentages.
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
- Pin observed dataset coverage with `coverage_fingerprint` on every relevant
  dataset case and step. After a run, inspect each case and step's covered files,
  lines, observed fingerprint, expected fingerprint, and baseline match in the
  runner output or `--coverage-report`. A mismatch fails the case and reports
  both expected and observed fingerprints. Review the scope's covered lines and
  whether it adds meaningful coverage before updating its dataset fingerprint;
  do not refresh fingerprints blindly. Sequence cases pin the combined step
  coverage, and each step may pin its own coverage independently.
- Implement predicate semantics in the Rust predicate contract and run its
  shared predicate dataset against that Rust implementation.
- Dogfood Open Contract when its Rust core can run and verify a repository
  contract through the same manifest, isolation, and verification path offered
  to consumers. Keep only the minimal bootstrap needed to reach that point.
- Collect nested-contract code coverage when the child runs as part of its
  parent. Combine parent and executed-child coverage in the root report, while
  reporting case results separately.

## Isolation

- **Bubblewrap** provides a restricted filesystem view and namespace isolation.
  Keep mounts limited to the contract and required runtime resources.
- **Landlock** limits filesystem access to allowed paths. It does not hide paths
  or restrict network access, so it is filesystem control rather than a complete
  sandbox.

Report isolation failures clearly and describe each mechanism's limits.
