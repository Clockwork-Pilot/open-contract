PYTHON ?= python3
CARGO ?= cargo
CARGO_TARGET_DIR ?= $(HOME)/.cache/open-contract-target
export CARGO_TARGET_DIR
LLVM_PROFILE_FILE ?= $(CURDIR)/.coverage/bootstrap-contracts/%m-%p.profraw
export LLVM_PROFILE_FILE
COVERAGE_PROJECT ?=
COVERAGE_PROFILE ?=
COVERAGE_IGNORE_REGEX ?= ^(/|tests/)
COVERAGE_HTML_DIR ?=
COVERAGE_SUMMARY ?=
COVERAGE_BINARY_PATH ?=
COVERAGE_BINARY ?= $(shell cat "$(COVERAGE_BINARY_PATH)" 2>/dev/null)
LLVM_COV ?= $(shell PATH="$(HOME)/.cargo/bin:$(PATH)" rustc --print sysroot)/lib/rustlib/$(shell PATH="$(HOME)/.cargo/bin:$(PATH)" rustc -vV | sed -n 's/^host: //p')/bin/llvm-cov

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-rust-core-contracts check-manifest-contract check-isolation-contract check-batch-execution-contract check-verification-contract check-error-handling-contract check-dataset-generation-contract check-predicate-contract check-bootstrap-dataset-harness check-rust-runtime-safety check-runner-examples check-python-predicate-example bootstrap-contract-coverage-html check-bootstrap-contract-coverage-html check-bootstrap-contracts-html check-bootstrap-dataset-harness-html check-manifest-contract-html check-isolation-contract-html check-batch-execution-contract-html check-verification-contract-html check-error-handling-contract-html check-dataset-generation-contract-html check-predicate-contract-html examples

check-contracts: check-bootstrap-contracts check-runner-examples check-python-predicate-example

check-bootstrap-contracts: check-predicate-contract check-bootstrap-dataset-harness check-rust-core-contracts

check-rust-core-contracts: check-rust-runtime-safety check-manifest-contract check-isolation-contract check-batch-execution-contract check-verification-contract check-error-handling-contract check-dataset-generation-contract

check-manifest-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/manifest

check-isolation-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/isolation

check-batch-execution-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/batch-execution

check-verification-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/verification

check-error-handling-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/error-handling

check-dataset-generation-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/dataset-generation

bootstrap-contract-coverage-html:
	rm -rf "$(COVERAGE_HTML_DIR)"
	mkdir -p "$(COVERAGE_HTML_DIR)"
	cd "$(COVERAGE_PROJECT)" && $(LLVM_COV) show --format=html --show-instantiation-summary --output-dir="$(abspath $(COVERAGE_HTML_DIR))" --instr-profile="$(abspath $(COVERAGE_PROFILE))" --ignore-filename-regex="$(COVERAGE_IGNORE_REGEX)" "$(COVERAGE_BINARY)"
	cd "$(COVERAGE_PROJECT)" && $(LLVM_COV) report --show-instantiation-summary --show-region-summary --instr-profile="$(abspath $(COVERAGE_PROFILE))" --ignore-filename-regex="$(COVERAGE_IGNORE_REGEX)" "$(COVERAGE_BINARY)" > "$(abspath $(COVERAGE_SUMMARY))"
	cat "$(COVERAGE_SUMMARY)"
	@printf 'HTML coverage report: %s/index.html\n' "$(COVERAGE_HTML_DIR)"

check-bootstrap-contract-coverage-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh batch-execution dataset-generation dataset-harness error-handling isolation manifest predicate verification

check-bootstrap-contracts-html: check-bootstrap-contract-coverage-html

check-bootstrap-dataset-harness-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh dataset-harness

check-batch-execution-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh batch-execution

check-dataset-generation-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh dataset-generation

check-error-handling-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh error-handling

check-isolation-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh isolation

check-manifest-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh manifest

check-predicate-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh predicate

check-verification-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh verification

check-predicate-contract:
	$(MAKE) -C runner/bootstrap-contracts/predicates check-predicate-contract

check-bootstrap-dataset-harness:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/dataset-harness
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) test --manifest-path runner/bootstrap-contracts/dataset-harness/Cargo.toml

check-rust-runtime-safety:
	$(PYTHON) runner/tools/check_rust_runtime_safety.py

check-runner-examples:
	$(MAKE) -C contract-languages/python test PYTHON="$(PYTHON)"
	cd contract-languages/javascript && npm test

check-python-predicate-example:
	$(MAKE) -C examples/python check PYTHON="$(PYTHON)"

examples: check-python-predicate-example
