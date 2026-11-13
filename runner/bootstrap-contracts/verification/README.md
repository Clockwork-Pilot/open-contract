# Output verification bootstrap contract

This package's dataset exercises the Rust runner's expected-output matching
and predicate verification after contract execution. The Rust dataset
executable is `src/bin/dataset-contract.rs`; coverage comes from running its
dataset, not from unit tests.

The Rust runner verifies validator outputs. Contract-language support under
`contract-languages/` supplies the validator interface and runtime; it does not
decide whether outputs satisfy dataset expectations or predicates.

Run the dataset coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/verification
```
