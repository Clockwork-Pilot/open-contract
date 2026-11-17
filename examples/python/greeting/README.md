# Python contract example

This example uses the Python language adapter to pass a small dataset of real
inputs to a contract-owned greeting validator. The Rust runner checks both the
successful result and the validator's input error, and enforces the manifest's
100% dataset line coverage requirement.

The sibling [predicate example](../predicate/README.md) exercises a Python
predicate evaluator with a broader dataset.

Run the contract through the Rust runner:

```sh
make -C examples/python/greeting check
```

Set `OPEN_CONTRACT_ISOLATION=landlock` or `OPEN_CONTRACT_ISOLATION=bwrap` to
select an available backend. The same setting is used by `make check-contracts`
from the repository root.

The manifest declares the validator and dataset as contract files. The command
imports `run_dataset` from the Python language adapter provided by Open Contract.
