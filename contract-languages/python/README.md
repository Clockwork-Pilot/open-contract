# Python contract-language support

This package provides the Python batch interface used by a contract-owned
entrypoint. It is not a contract runner: it does not parse manifests, create
isolation, compare expected values, or evaluate predicates.

A contract command imports `run_dataset`, supplies a loader for its own
`validate(input)` function, and returns the resulting JSON object to the Rust
runner. `run_dataset` iterates the flattened batch once, preserves each
contract/case/step key, captures validator errors as results, and reports
Python coverage. Rust verifies expected values and predicates after commands
finish.

```python
import json
import os
import sys

sys.path.insert(0, os.environ["OPEN_CONTRACT_LANGUAGE_PATH"])
from open_contract_language.batch_adapter import run_dataset


def load_validator():
    from validator import validate
    return validate


request = json.load(sys.stdin)
print(json.dumps(run_dataset(request, validator_loader=load_validator)))
```

The manifest carries only the logical `language_package` identifier and the
contract's `execution.command`. Runner configuration maps that identifier to
the support package root; the runner provides its isolated location through
`OPEN_CONTRACT_LANGUAGE_PATH`. No support file list or host path belongs in the
manifest.

Run the interface tests with `python3 -m pytest` from this directory. They test
the adapter protocol and are separate from contract dataset coverage.
