# Filesystem isolation bootstrap contract

This package's dataset exercises the Rust runner's filesystem staging and
isolation behavior. The dataset executable is `src/main.rs`.
Its cases check declared files, writes, nested package files, and fresh staging
roots. Dataset coverage comes from running those cases.

The dataset currently covers at least 90% of executable implementation lines.
The manifest declares one supplemental test file for Landlock syscall paths
that cannot be triggered safely and portably through dataset cases. Its coverage
is reported separately and combined only over lines executable in the dataset
build; ordinary unit tests do not count.

Isolation belongs to the Rust runner. A contract-language implementation must
not establish or weaken the boundary; language support is separate under
`contract-languages/`. The contract manifest describes package files, while the
runner applies the selected isolation backend.

Run the dataset coverage check from the repository root:

```sh
python3 runner/tools/check_bootstrap_contract_coverage.py runner/bootstrap-contracts/isolation
```
