PYTHON ?= python3
CARGO ?= cargo
OPEN_CONTRACT_ISOLATION ?= auto

.PHONY: check check-greeting check-predicate

check: check-greeting check-predicate

check-greeting:
	$(MAKE) -C greeting check PYTHON="$(PYTHON)" CARGO="$(CARGO)" OPEN_CONTRACT_ISOLATION="$(OPEN_CONTRACT_ISOLATION)"

check-predicate:
	$(MAKE) -C predicate check PYTHON="$(PYTHON)" CARGO="$(CARGO)" OPEN_CONTRACT_ISOLATION="$(OPEN_CONTRACT_ISOLATION)"
