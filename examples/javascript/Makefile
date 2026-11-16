NPM ?= npm
CARGO ?= cargo
ISOLATION ?= none

.PHONY: check install-adapter check-open-contract

check: install-adapter check-open-contract

install-adapter:
	$(NPM) ci --prefix ../../contract-languages/javascript

check-open-contract:
	PATH="$(HOME)/.cargo/bin:$(PATH)" $(CARGO) run --manifest-path ../../runner/rust/Cargo.toml -- run-manifest contract.yaml --isolation $(ISOLATION) --coverage-report ../../.coverage/examples/javascript.json
