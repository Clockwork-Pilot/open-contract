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
.PHONY: check-contracts check-bootstrap-contracts check-rust-core-contracts check-manifest-contract check-isolation-contract check-batch-execution-contract check-verification-contract check-error-handling-contract check-dataset-generation-contract check-predicate-contract check-bootstrap-dataset-harness check-rust-runtime-safety check-runner-examples check-python-predicate-example bootstrap-contract-coverage-html check-bootstrap-contract-coverage-html check-manifest-contract-html check-isolation-contract-html check-batch-execution-contract-html check-verification-contract-html check-error-handling-contract-html check-dataset-generation-contract-html check-predicate-contract-html examples

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
	@$(MAKE) check-bootstrap-contracts; check_status=$$?; \
	$(MAKE) -k report-batch-execution-contract-html report-dataset-generation-contract-html report-error-handling-contract-html report-isolation-contract-html report-manifest-contract-html report-predicate-contract-html report-verification-contract-html; report_status=$$?; \
	exit $$((check_status || report_status))

define bootstrap_contract_html_target
.PHONY: report-$(1)-contract-html check-$(1)-contract-html
report-$(1)-contract-html:
	$$(MAKE) bootstrap-contract-coverage-html COVERAGE_PROJECT=runner/bootstrap-contracts/$(2) COVERAGE_PROFILE=$$(abspath $(3)) COVERAGE_BINARY_PATH=$$(abspath .coverage/bootstrap-contracts/$(2)-dataset-binary.path) COVERAGE_HTML_DIR=$$(abspath .coverage/bootstrap-contracts/$(2)/html) COVERAGE_SUMMARY=$$(abspath .coverage/bootstrap-contracts/$(2)/summary.txt)
check-$(1)-contract-html:
	@set +e; \
	$$(MAKE) check-$(1)-contract; check_status=$$$$?; \
	$$(MAKE) report-$(1)-contract-html; report_status=$$$$?; \
	exit $$$$((check_status || report_status))
endef

$(eval $(call bootstrap_contract_html_target,batch-execution,batch-execution,.coverage/bootstrap-contracts/batch-execution-dataset.profdata))
$(eval $(call bootstrap_contract_html_target,dataset-generation,dataset-generation,.coverage/bootstrap-contracts/dataset-generation-dataset.profdata))
$(eval $(call bootstrap_contract_html_target,error-handling,error-handling,.coverage/bootstrap-contracts/error-handling-dataset.profdata))
$(eval $(call bootstrap_contract_html_target,isolation,isolation,.coverage/bootstrap-contracts/isolation-dataset.profdata))
$(eval $(call bootstrap_contract_html_target,manifest,manifest,.coverage/bootstrap-contracts/manifest-dataset.profdata))
$(eval $(call bootstrap_contract_html_target,predicate,predicates,$(HOME)/.cache/open-contract-target/predicates/llvm-cov-target/predicates.profdata))
$(eval $(call bootstrap_contract_html_target,verification,verification,.coverage/bootstrap-contracts/verification-dataset.profdata))

check-predicate-contract:
	$(MAKE) -C runner/bootstrap-contracts/predicates check-predicate-contract

check-bootstrap-dataset-harness:
	mkdir -p .coverage/bootstrap-contracts
	$(CARGO) test --manifest-path runner/bootstrap-contracts/dataset-harness/Cargo.toml

check-rust-runtime-safety:
	$(PYTHON) runner/tools/check_rust_runtime_safety.py

check-runner-examples:
	$(MAKE) -C contract-languages/python test PYTHON="$(PYTHON)"
	cd contract-languages/javascript && npm test

check-python-predicate-example:
	$(MAKE) -C examples/python check PYTHON="$(PYTHON)"

examples: check-python-predicate-example
