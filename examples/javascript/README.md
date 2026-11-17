# JavaScript contract example

This small contract runs two real inputs through the JavaScript language
adapter: a greeting result and a validation error. Open Contract's Rust runner
loads the manifest, invokes the contract command, checks the results and
enforces dataset coverage.

Run it from the Open Contract root with:

```sh
make check-javascript-example
```

Set `OPEN_CONTRACT_ISOLATION=landlock` (or `bwrap`, `tempdir`, or `none`) to
select the runner mode. The setting also applies to the other runner-backed
examples when running `make check-contracts`.

The goal installs the adapter's locked npm dependencies before running the
contract. The example report is written under `.coverage/examples/`.
