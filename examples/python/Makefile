PYTHON ?= python3
CARGO ?= cargo
ISOLATION ?= none

.PHONY: check check-greeting check-predicate

check: check-greeting check-predicate

check-greeting:
	$(MAKE) -C greeting check PYTHON="$(PYTHON)" CARGO="$(CARGO)" ISOLATION="$(ISOLATION)"

check-predicate:
	$(MAKE) -C predicate check PYTHON="$(PYTHON)" CARGO="$(CARGO)" ISOLATION="$(ISOLATION)"
