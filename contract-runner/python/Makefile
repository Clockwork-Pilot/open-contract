PYTHON ?= python3
.PHONY: examples example-raw-dataset example-ingested-dataset validate-raw-dataset test

examples: example-raw-dataset example-ingested-dataset

example-raw-dataset: validate-raw-dataset
	$(PYTHON) -m open_contracts_runner run examples/raw-dataset/dataset.json --executor evaluator:evaluate --quiet

validate-raw-dataset:
	$(PYTHON) examples/raw-dataset/validate_schema.py examples/raw-dataset/dataset.json

example-ingested-dataset:
	$(PYTHON) -m open_contracts_runner run examples/ingested-dataset/input.json --reader ingestor:read_dataset --executor evaluator:evaluate --quiet

test:
	$(PYTHON) -m pytest
