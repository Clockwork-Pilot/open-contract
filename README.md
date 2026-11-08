# Open Contract

Open Contract provides dataset-driven tools for testing portable API contracts.
The repository includes the Python contract runner, JavaScript runner, and
bootstrap predicate contract.

## Running contract checks in Bubblewrap

The Python runner can execute a dataset with Bubblewrap isolation. Mount the
repository's executable `contracts/` directory as the contract root; the runner
mounts its own checkout separately and exposes any additional dependencies only
when they are declared. Contract sources and dependencies are read-only inside
the sandbox. The contract source directory and working directory use a fresh
randomized mount root; the runner and output mount have stable internal paths.
Coverage data is copied through the output mount to the caller's requested path.

```sh
PYTHONPATH="/path/to:/path/to/open-contract/contract-runner/python/src:/path/to/open-contract/bootstrap-contracts/predicates/python/src" \
  python -m open_contracts_runner run \
  /path/to/contracts/example/datasets/dataset.json \
  --executor contracts.example.runner:evaluate \
  --isolation bubblewrap \
  --contract-root /path/to/contracts \
  --coverage --coverage-file /tmp/example.coverage --quiet
```

Use `--sandbox-dependency PATH` for each external source dependency that the
contract must import. Dependencies are mounted read-only and added to
`PYTHONPATH`. The runner's Python environment and standard runtime libraries
are also mounted read-only. The sandbox clears inherited environment variables,
disables network access, and exposes no other project or home-directory paths.

The host needs Bubblewrap with namespace support. If `bwrap` is missing or the
kernel blocks namespace creation, the runner reports an isolation error and
does not fall back to an unsandboxed run.

The CWPilot Playbook contracts Makefile enables this runner mode by default;
`CONTRACT_SANDBOX_DEPENDENCIES` accepts a space-separated list of read-only
dependency roots.
