PYTHON ?= python3
CARGO ?= cargo
OPEN_CONTRACT_ISOLATION ?= auto

.PHONY: check

check:
	$(MAKE) -C ../../../contract-languages/python install-dependencies PYTHON="$(PYTHON)"
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) run --manifest-path ../../../runner/rust/Cargo.toml -- run-manifest contract.yaml --isolation $(OPEN_CONTRACT_ISOLATION) --coverage-report ../../../.coverage/examples/python-predicate.json
