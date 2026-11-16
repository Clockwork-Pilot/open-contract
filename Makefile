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
COVERAGE_SOURCE_FILTER ?=
LLVM_COV ?= $(shell PATH="$(HOME)/.cargo/bin:$(PATH)" rustc --print sysroot)/lib/rustlib/$(shell PATH="$(HOME)/.cargo/bin:$(PATH)" rustc -vV | sed -n 's/^host: //p')/bin/llvm-cov
LLVM_PROFDATA ?= $(patsubst %/,%,$(dir $(LLVM_COV)))/llvm-profdata

.DEFAULT_GOAL := check-contracts
.PHONY: check-contracts check-bootstrap-contracts check-rust-core-contracts check-cli-contract check-manifest-contract check-isolation-contract check-batch-execution-contract check-verification-contract check-error-handling-contract check-dataset-generation-contract check-cli-contract check-predicate-contract check-bootstrap-dataset-harness check-rust-runtime-safety check-runner-examples check-language-contract-examples check-rust-language-adapter check-rust-nested-contracts check-rust-nested-contracts-html check-python-example check-javascript-example bootstrap-contract-coverage-html check-bootstrap-contract-coverage-html check-bootstrap-contracts-html check-bootstrap-dataset-harness-html check-manifest-contract-html check-isolation-contract-html check-batch-execution-contract-html check-verification-contract-html check-error-handling-contract-html check-dataset-generation-contract-html check-cli-contract-html check-predicate-contract-html examples

check-contracts: check-bootstrap-contracts check-language-contract-examples

check-bootstrap-contracts: check-predicate-contract check-bootstrap-dataset-harness check-rust-core-contracts

check-rust-core-contracts: check-rust-runtime-safety check-manifest-contract check-isolation-contract check-batch-execution-contract check-verification-contract check-error-handling-contract check-dataset-generation-contract check-cli-contract

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
	cd "$(COVERAGE_PROJECT)" && $(LLVM_COV) show --format=html --show-instantiation-summary --output-dir="$(abspath $(COVERAGE_HTML_DIR))" --instr-profile="$(abspath $(COVERAGE_PROFILE))" --ignore-filename-regex="$(COVERAGE_IGNORE_REGEX)" $(COVERAGE_SOURCE_FILTER) "$(COVERAGE_BINARY)"
	cd "$(COVERAGE_PROJECT)" && $(LLVM_COV) report --show-instantiation-summary --show-region-summary --instr-profile="$(abspath $(COVERAGE_PROFILE))" --ignore-filename-regex="$(COVERAGE_IGNORE_REGEX)" $(COVERAGE_SOURCE_FILTER) "$(COVERAGE_BINARY)" > "$(abspath $(COVERAGE_SUMMARY))"
	cat "$(COVERAGE_SUMMARY)"
	@printf 'HTML coverage report: %s/index.html\n' "$(COVERAGE_HTML_DIR)"

check-bootstrap-contract-coverage-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh batch-execution cli dataset-generation dataset-harness error-handling isolation manifest predicate verification

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

check-cli-contract-html:
	sh ./runner/tools/check_bootstrap_contract_html.sh cli

check-predicate-contract:
	$(MAKE) -C runner/bootstrap-contracts/predicates check-predicate-contract

check-bootstrap-dataset-harness:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/dataset-harness
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) test --manifest-path runner/bootstrap-contracts/dataset-harness/Cargo.toml

check-rust-runtime-safety:
	$(PYTHON) runner/tools/check_rust_runtime_safety.py

check-runner-examples:
	$(MAKE) -C contract-languages/python test PYTHON="$(PYTHON)"
	$(MAKE) -C contract-languages/javascript test
	$(MAKE) check-javascript-example
	$(MAKE) check-rust-language-adapter
	$(MAKE) check-rust-nested-contracts

check-language-contract-examples: check-runner-examples check-python-example

check-rust-language-adapter:
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) test --manifest-path contract-languages/rust/Cargo.toml

check-rust-nested-contracts:
	PATH="$(HOME)/.cargo/bin:$(PATH)" sh examples/rust/check.sh

check-rust-nested-contracts-html: check-rust-nested-contracts
	$(LLVM_PROFDATA) merge -sparse .coverage/rust-nested-target/profiles/*.profraw -o .coverage/rust-nested.profdata
	$(MAKE) bootstrap-contract-coverage-html COVERAGE_PROJECT=. COVERAGE_PROFILE=.coverage/rust-nested.profdata COVERAGE_BINARY="$(abspath .coverage/rust-nested-target/parent/debug/contract-command)" COVERAGE_HTML_DIR=.coverage/rust-nested/parent/html COVERAGE_SUMMARY=.coverage/rust-nested/parent/summary.txt COVERAGE_IGNORE_REGEX='.cargo/|/rustc/|contract-languages/rust/src/|/main.rs'
	$(MAKE) bootstrap-contract-coverage-html COVERAGE_PROJECT=. COVERAGE_PROFILE=.coverage/rust-nested.profdata COVERAGE_BINARY="$(abspath .coverage/rust-nested-target/child/debug/contract-command)" COVERAGE_HTML_DIR=.coverage/rust-nested/child/html COVERAGE_SUMMARY=.coverage/rust-nested/child/summary.txt COVERAGE_IGNORE_REGEX='.cargo/|/rustc/|contract-languages/rust/src/|/main.rs'

check-python-example:
	$(MAKE) -C examples/python check PYTHON="$(PYTHON)"

check-javascript-example:
	$(MAKE) -C examples/javascript check

examples: check-language-contract-examples

check-cli-contract:
	$(PYTHON) runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/cli
