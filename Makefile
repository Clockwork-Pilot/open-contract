PYTHON ?= python3
CARGO ?= cargo
CARGO_TARGET_DIR ?= $(HOME)/.cache/open-contract-target
export CARGO_TARGET_DIR

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-rust-core-contracts check-rust-runtime-safety check-runner-examples check-python-predicate-example examples

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
	$(MAKE) -C contract-languages/python test PYTHON="$(PYTHON)"
	cd contract-languages/javascript && npm test

check-python-predicate-example:
	$(MAKE) -C examples/python check PYTHON="$(PYTHON)"

examples: check-python-predicate-example
