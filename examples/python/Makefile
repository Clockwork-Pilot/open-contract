PYTHON ?= python3
CARGO ?= cargo
ISOLATION ?= none

.PHONY: check check-dataset check-open-contract

check: check-dataset check-open-contract

check-dataset:
	$(PYTHON) check_coverage.py

check-open-contract:
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) run --manifest-path ../../runner/rust/Cargo.toml -- run-manifest contract.yaml --isolation $(ISOLATION)
