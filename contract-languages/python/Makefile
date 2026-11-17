PYTHON ?= python3
.PHONY: install-dependencies test

.deps/.runtime-installed: requirements.txt
	$(PYTHON) -m pip install --disable-pip-version-check --no-warn-script-location --upgrade --target .deps -r requirements.txt
	touch $@

install-dependencies: .deps/.runtime-installed

test:
	$(PYTHON) -m pytest
