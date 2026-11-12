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
  `bootstrap-contracts`: one each for manifest parsing and planning, filesystem
  isolation, batch dataset and step execution with coverage collection, and
  post-batch expected-output and predicate verification. Use these to exercise
  the corresponding Rust layers as the core develops.
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
