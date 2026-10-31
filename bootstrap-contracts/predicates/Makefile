PREDICATE_DIR := .
PYTHON := python3
.DEFAULT_GOAL := check-predicate-contract

.PHONY: generate-dataset check-generated-dataset check-predicate-contract check-python-predicate-contract check-rust-predicate-contract \
        check-predicate-contract-coverage check-python-predicate-contract-coverage \
        check-rust-predicate-contract-coverage

# Contract checks run their datasets under coverage and require 100% evaluator coverage.
generate-dataset:
	$(PYTHON) generate_dataset.py

check-generated-dataset:
	$(PYTHON) generate_dataset.py --check

check-predicate-contract: check-generated-dataset check-python-predicate-contract check-rust-predicate-contract

check-predicate-contract-coverage: check-predicate-contract

check-python-predicate-contract:
	cd $(PREDICATE_DIR)/python && PYTHONPATH=src $(PYTHON) check_coverage.py

check-python-predicate-contract-coverage: check-python-predicate-contract

check-rust-predicate-contract:
	$(PYTHON) $(PREDICATE_DIR)/rust/check_coverage.py

check-rust-predicate-contract-coverage: check-rust-predicate-contract
