PYTHON ?= python3
CARGO_TARGET_DIR ?= $(HOME)/.cache/open-contract-target
RUNNER_SRC := $(CURDIR)/contract-languages/python/src
PREDICATE_SRC := $(CURDIR)/examples/python
export PYTHONPATH := $(RUNNER_SRC):$(PREDICATE_SRC)$(if $(PYTHONPATH),:$(PYTHONPATH))
export CARGO_TARGET_DIR

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-rust-core-contracts check-rust-runtime-safety check-runner-examples check-python-predicate-example examples example-raw-dataset example-ingested-dataset

check-contracts: check-bootstrap-contracts check-runner-examples check-python-predicate-example

check-bootstrap-contracts:
	$(MAKE) -C runner/bootstrap-contracts/predicates check-predicate-contract
	$(MAKE) check-rust-core-contracts

check-rust-core-contracts: check-rust-runtime-safety
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/manifest
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/isolation
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/batch-execution
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/verification
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/error-handling

check-rust-runtime-safety:
	$(PYTHON) runner/tools/check_rust_runtime_safety.py

check-runner-examples:
	$(MAKE) -C contract-languages/python check-examples-coverage PYTHON="$(PYTHON)"

check-python-predicate-example:
	$(MAKE) -C examples/python check PYTHON="$(PYTHON)"

examples: example-raw-dataset example-ingested-dataset check-python-predicate-example

example-raw-dataset:
	$(MAKE) -C contract-languages/python example-raw-dataset PYTHON="$(PYTHON)"

example-ingested-dataset:
	$(MAKE) -C contract-languages/python example-ingested-dataset PYTHON="$(PYTHON)"
