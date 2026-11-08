PYTHON ?= python3
RUNNER_SRC := $(CURDIR)/contract-runner/python/src
PREDICATE_SRC := $(CURDIR)/bootstrap-contracts/predicates/python/src
export PYTHONPATH := $(RUNNER_SRC):$(PREDICATE_SRC)$(if $(PYTHONPATH),:$(PYTHONPATH))

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-runner-examples examples example-raw-dataset example-ingested-dataset

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
