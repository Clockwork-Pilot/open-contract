PYTHON ?= python3
RUNNER_SRC := $(CURDIR)/contract-runner/python/src
PREDICATE_SRC := $(CURDIR)/bootstrap-contracts/predicates/python/src
export PYTHONPATH := $(RUNNER_SRC):$(PREDICATE_SRC)$(if $(PYTHONPATH),:$(PYTHONPATH))

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-runner-examples examples example-raw-dataset example-ingested-dataset contract-worktree-create contract-worktree-remove test-contract-worktree

check-contracts: check-bootstrap-contracts check-runner-examples

check-bootstrap-contracts:
	$(MAKE) -C bootstrap-contracts/predicates check-predicate-contract

check-runner-examples:
	$(MAKE) -C contract-runner/python check-examples-coverage PYTHON="$(PYTHON)"

examples: example-raw-dataset example-ingested-dataset

example-raw-dataset:
	$(MAKE) -C contract-runner/python example-raw-dataset PYTHON="$(PYTHON)"

example-ingested-dataset:
	$(MAKE) -C contract-runner/python example-ingested-dataset PYTHON="$(PYTHON)"

contract-worktree-create:
	CONTRACT_REPO="$(CONTRACT_REPO)" CONTRACT_BRANCH="$(CONTRACT_BRANCH)" WORKTREE_PATH="$(WORKTREE_PATH)" bash scripts/contract-worktree.sh create

contract-worktree-remove:
	WORKTREE_PATH="$(WORKTREE_PATH)" bash scripts/contract-worktree.sh remove

test-contract-worktree:
	python3 scripts/test-contract-worktree.py
