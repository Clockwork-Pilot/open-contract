# Raw dataset

This example starts with cases already in the runner's normalized dataset
format. Each case has an `id`, `input`, and either an `expected` result or a
`predicate` evaluated against the observation.

`evaluator.py` processes one complete case. `validate_schema.py` checks the
dataset against the bundle schema.

From the repository root, run the raw dataset directly with:

Install the runtime dependencies first with `python3 -m pip install -r
contract-runner/python/requirements.txt`.

```sh
PYTHONPATH=contract-runner/python/src:bootstrap-contracts/predicates/python/src \
python3 -m open_contracts_runner run \
  contract-runner/python/examples/raw-dataset/dataset.json \
  --executor evaluator:evaluate --quiet
```
