# Python contract-language support

This package provides the Python batch interface used by a contract-owned
entrypoint. It is not a contract runner: it does not parse manifests, create
isolation, compare expected values, or evaluate predicates.

A contract command imports `run_dataset`, supplies a loader for its own
`validate(input, context)` function, and returns the resulting JSON object to
the Rust runner. Context contains `case_id`, optional `step_id`, optional
`setup`, and `parallel`. `run_dataset` preserves each request key, captures
validator errors as results, and reports Python coverage. Adjacent
`parallel: true` steps in one case run concurrently and share a coverage scope.
Rust verifies expected values and predicates after commands finish.

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

Run the interface tests with `make -C contract-languages/python test` from the
`open-contract` directory. They test the adapter protocol and are separate
from contract dataset coverage.

Install the adapter's runtime dependencies into its isolated package directory
with `make -C contract-languages/python install-dependencies PYTHON=python3`.
`coverage.py` belongs to this adapter: its dependencies stay in `.deps/` and
are exposed by this package's `runtime.yaml`. Python adapter checks and contracts that declare this adapter use this
adapter-owned runtime. Other language adapters and the Rust runner itself do
not require these Python dependencies.
