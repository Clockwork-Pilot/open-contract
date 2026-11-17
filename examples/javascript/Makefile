NPM ?= npm
CARGO ?= cargo
OPEN_CONTRACT_ISOLATION ?= auto

.PHONY: check install-adapter check-open-contract

check: install-adapter check-open-contract

install-adapter:
	$(NPM) ci --prefix ../../contract-languages/javascript

check-open-contract:
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) run --manifest-path ../../runner/rust/Cargo.toml -- run-manifest contract.yaml --isolation $(OPEN_CONTRACT_ISOLATION) --coverage-report ../../.coverage/examples/javascript.json
