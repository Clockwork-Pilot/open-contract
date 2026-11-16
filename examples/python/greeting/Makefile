PYTHON ?= python3
CARGO ?= cargo
ISOLATION ?= none

.PHONY: check check-open-contract

check: check-open-contract

check-open-contract:
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) run --manifest-path ../../../runner/rust/Cargo.toml -- run-manifest contract.yaml --isolation $(ISOLATION) --coverage-report ../../../.coverage/examples/python.json
