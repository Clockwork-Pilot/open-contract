# Open Contract

Open Contract provides dataset-driven tools for testing portable API contracts.
The repository includes the Python contract runner, JavaScript runner, and
bootstrap predicate contract.

## Developing another contract repository

Use a Git worktree to edit a contract repository without changing its stable
checkout:

```sh
make contract-worktree-create \
  CONTRACT_REPO=/path/to/contract-repo \
  CONTRACT_BRANCH=feature/my-contract-change \
  WORKTREE_PATH=/path/to/worktrees/my-contract-change
```

The branch is created from the stable checkout's current `HEAD` if it does not
exist. If the branch already exists, the worktree checks it out as-is. The
helper refuses an existing destination and any destination inside the stable
checkout. The worktree stays in place for editing, checks, and commits.

Run the contract repository's checks from the linked checkout, for example:

```sh
make -C /path/to/worktrees/my-contract-change check-contracts
```

To remove the linked checkout after committing:

```sh
make contract-worktree-remove WORKTREE_PATH=/path/to/worktrees/my-contract-change
```

Removal refuses the primary checkout and refuses worktrees with modified,
staged, untracked, or ignored files. Move or commit the files before trying
again. The helper never force-removes a worktree, and removing a clean worktree
does not delete its branch or commits. If removal fails, inspect `git status`
and `git worktree list`; the worktree remains available for recovery.

The directory `knowledge_tool/playbook` inside the CWPilot checkout is a Git
submodule, not a separately isolated contract repository. A linked worktree can
separate Git edits, but it does not restrict filesystem access. A runner with
access to the parent checkout can still read it. For runner isolation, execute
checks in a container or other environment that exposes only the contract
worktree and its declared dependencies.

Run the lifecycle checks with:

```sh
make test-contract-worktree
```
