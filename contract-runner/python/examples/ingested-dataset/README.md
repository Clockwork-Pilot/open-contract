# Ingested dataset

This example starts with native check records in `input.json`. `ingestor.py`
maps `case` to `id`, `request` to `input`, `result` to `expected`, and
`satisfies` to `predicate`. It preserves the authored exact result and
predicate; it does not create either one. `evaluator.py` processes each
normalized case.

From the repository root, run the source data through its reader and evaluator:

Install the runtime dependencies first with `python3 -m pip install -r
contract-runner/python/requirements.txt`.

```sh
PYTHONPATH=contract-runner/python/src:bootstrap-contracts/predicates/python/src \
python3 -m open_contracts_runner run \
  contract-runner/python/examples/ingested-dataset/input.json \
  --reader ingestor:read_dataset --executor evaluator:evaluate --quiet
```
