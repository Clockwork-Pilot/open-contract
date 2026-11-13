# Error handling bootstrap contract

This contract exercises the Rust runner's shared error value: nonzero exit-code
safety, layer context, and conversion of existing layer errors. The Rust
runner uses this type at its command boundary so failures from manifest,
isolation, batch execution, and verification can retain their layer context and
produce an actionable message without panicking.

The dataset is the coverage source for `src/lib.rs`; ordinary unit-test coverage
is not included. Run its dataset coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/error-handling
```
