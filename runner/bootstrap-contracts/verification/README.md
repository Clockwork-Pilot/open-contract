# Output verification bootstrap contract

This package's dataset exercises the public `match_output` function. Within
`input`, `actual`, `expected`, and optional `predicate` are the function
arguments. The case-level `expected` records the function's actual result:
`"expected"`, `"expected_and_predicate"`, or an `{ "error": ... }` object.
This keeps the matcher's expected argument distinct from its returned result.
The Rust dataset executable is `src/main.rs`; coverage comes from running its
dataset, not from unit tests.

The Rust runner verifies validator outputs. Contract-language support under
`contract-languages/` supplies the validator interface and runtime; it does not
decide whether outputs satisfy dataset expectations or predicates.

Run only this contract and its required dataset coverage checker from the
Open Contract repository root:

```sh
make check-verification-contract
```
