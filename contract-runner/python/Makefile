PYTHON ?= python3
.PHONY: examples example-raw-dataset example-ingested-dataset check-examples-coverage test

examples: example-raw-dataset example-ingested-dataset

example-raw-dataset:
	$(PYTHON) -m open_contracts_runner run examples/raw-dataset/dataset.json --executor evaluator:evaluate --quiet

example-ingested-dataset:
	$(PYTHON) -m open_contracts_runner run examples/ingested-dataset/input.json --reader ingestor:read_dataset --executor evaluator:evaluate --quiet

check-examples-coverage:
	$(PYTHON) -m open_contracts_runner run examples/raw-dataset/dataset.json --executor evaluator:evaluate --coverage --coverage-file .coverage.examples --coverage-threshold 100 --quiet
	$(PYTHON) -m open_contracts_runner run examples/ingested-dataset/input.json --reader ingestor:read_dataset --executor evaluator:evaluate --coverage --coverage-append --coverage-file .coverage.examples --coverage-threshold 100 --quiet

test:
	$(PYTHON) -m pytest
