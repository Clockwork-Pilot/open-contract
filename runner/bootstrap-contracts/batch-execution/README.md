# Batch execution bootstrap contract

This bootstrap contract exercises the batch runner's public functions with
inputs from `dataset.json`. The validator builds the manifest files required by
each case, invokes the public API, and compares its actual result with the
case's expectation.

Dataset coverage is the required coverage source. The scoped checker requires
100% dataset function, instantiation, line, and region coverage, and requires
every case to pin its observed 32-character fingerprint. Unit tests may cover
internal helper behavior that the public interface cannot expose, but their
coverage does not fill dataset gaps.

When a case needs an external condition, mark it with `setup: true`. The
validator maps the case ID to setup such as a controlled adapter executable,
then passes the case's actual input through the public batch API and checks the
returned result. Setup does not supply the expected result or call private
implementation helpers. Fixture controls stay outside `input`.

For consumer contracts, the Rust runner reads the manifest, starts the selected
isolation boundary, executes its declared language-adapter command once for the
contract tree, and verifies returned outputs afterward. Language adapters only
execute validators and report results and coverage; they do not read datasets,
iterate cases or steps, evaluate predicates, or provide isolation. That runtime
path is exercised when running consumer contracts. The batch bootstrap
contract focuses on public batch behavior. Unit tests are reserved for internal
behaviors the public batch interface cannot expose.

Datasets use format version 1 and may be JSON or YAML. Prefer YAML for
hand-authored datasets. Cases and steps can specify exact expected output,
predicates, or an expected validator error. A sequence can also check its
aggregate output. Each case and step that pins `coverage_fingerprint` uses the
first 128 bits of SHA-256 over sorted package-relative covered files and lines.
Every case and step must declare a nonzero `coverage_fingerprint`; missing,
malformed, zero, and mismatched values fail. Repeated fingerprints are allowed:
they mean those inputs covered the same source lines, while still permitting
distinct behavior checks on those inputs. They do not increase the union of
covered source lines, so use the dataset coverage report to see whether new
cases add line coverage.

Run only this contract's dataset and coverage checker from the Open Contract
repository root:

```sh
make check-batch-execution-contract
```

The same target is included in the Rust core check, alongside the other Rust
bootstrap contracts:

```sh
make check-rust-core-contracts
```
