# Python predicate contract example

This example implements a predicate evaluator as a standalone Open Contract.
The dataset contains real predicate expressions and JSON values; it does not
exercise the bootstrap evaluator or rely on its implementation. The Python
language adapter runs each whole dataset entry through `validate`, and the Rust
runner checks expected results, per-case fingerprints, and 100% dataset line
coverage for the declared Python sources.

Run it from the Open Contract root:

```sh
make -C examples/python/predicate check
```

The coverage report is written to `.coverage/examples/python-predicate.json`.
