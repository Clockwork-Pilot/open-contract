# Raw dataset

This example starts with cases already in the runner's normalized dataset
format. Each case has an `id`, `input`, and either an `expected` result or a
`predicate` evaluated against the observation.

`evaluator.py` processes one complete case. Open Contracts validates the
dataset against its bundle and predicate schemas before executing its cases.

From the repository root, run the raw dataset directly with:

Install the runtime dependencies first with `python3 -m pip install -r
contract-languages/python/requirements.txt`.

```sh
PYTHONPATH=contract-languages/python/src:examples/python \
python3 -m open_contracts_runner run \
  contract-languages/python/examples/raw-dataset/dataset.json \
  --executor evaluator:evaluate --quiet
```
