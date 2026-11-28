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
from importlib.util import module_from_spec, spec_from_file_location

entrypoint = os.environ["OPEN_CONTRACT_LANGUAGE_ENTRYPOINT"]
spec = spec_from_file_location("open_contract_language.batch_adapter", entrypoint)
if spec is None or spec.loader is None:
    raise RuntimeError("cannot load language adapter entrypoint")
adapter = module_from_spec(spec)
sys.modules[spec.name] = adapter
spec.loader.exec_module(adapter)
run_dataset = adapter.run_dataset


def load_validator():
    from validator import validate
    return validate


request = json.load(sys.stdin)
print(json.dumps(run_dataset(request, validator_loader=load_validator)))
```

The contract manifest points `language_adapter` to the adapter's `runtime.yaml`.
That file declares its entrypoint, runtime files, and environment. The runner
exposes the resolved entrypoint as `OPEN_CONTRACT_LANGUAGE_ENTRYPOINT`; adapter
runtime files do not need duplicate entries in the contract's filesystem policy.

Run the interface tests with `make -C contract-languages/python test` from the
`open-contract` directory. They test the adapter protocol and are separate
from contract dataset coverage.

Install the adapter's runtime dependencies into its isolated package directory
with `make -C contract-languages/python install-dependencies PYTHON=python3`.
`coverage.py` belongs to this adapter: its dependencies stay in `.deps/` and
are installed under this adapter's `.deps/` directory. Adapter checks and
contract-owned commands can refer to those tools directly. The Rust runner
loads the runtime manifest named by `language_adapter` and makes its declared
files available inside isolation.

For declared Python sources, the adapter provides line, statement, and branch
arc entities with line locations from coverage.py. coverage.py does not provide
per-entity hit counts through this adapter, so those counts are reported as
unavailable. Function, region, and instantiation entities are not provided.
The Rust runner's `--coverage-html` option renders the available data.
