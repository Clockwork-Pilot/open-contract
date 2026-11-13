# Python predicate example

This example implements the predicate contract in Python. The dataset drives
the implementation, and Open Contract's Rust verifier checks each returned
boolean against its expected value.
The manifest uses the generic Python language adapter; the example has no
contract-specific batch runner.

Run both the dataset coverage check and the contract through the Rust runner:

```sh
make -C examples/python check
```

Set `ISOLATION=landlock` or `ISOLATION=bwrap` to select an available backend.

The dataset is copied from the Rust bootstrap conformance corpus and uses the
same predicate cases. The Python implementation is an example, not the bootstrap contract
implementation.
