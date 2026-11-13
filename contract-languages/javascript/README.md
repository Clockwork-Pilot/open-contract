# Open Contracts JavaScript runner

The JavaScript runner reads native Open Contract v1 JSON and YAML bundles. It
validates bundles with the shared schemas, runs built-in `command`, `hooks`,
and `http` targets, and matches JSON expected values or the shared predicate
language. `expected` is a recursive subset match for objects; arrays match by
length and position. A case may use both `expected` and `predicate` as
complementary checks. No project-specific reader is required for native Open
Contract bundles.

For compatibility with the Python runner, optional `--reader MODULE:CALLABLE`
adapts a parsed source document to an Open Contract bundle, and
`--executor MODULE:CALLABLE` handles targets whose kind is `custom`. Both
exports may be async JavaScript functions. Native v1 bundles and built-in
targets need neither option. Adapter paths resolve from the current working
directory; for example, `--executor ./evaluator.mjs:evaluate`.

For built-in `command` and `hooks` targets, set `config.cwd` to run the command
from a directory relative to the dataset file. An absolute `cwd` is used as-is;
when `cwd` is omitted, the runner keeps using its current working directory.
This lets a dataset keep its evaluator and other files in its own directory
without requiring the caller to start Open Contract from the repository root.

From the Open Contract repository root, run the example with:

```sh
cd contract-languages/javascript
npm install
npm test
node src/cli.js run examples/javascript/dataset.yaml \
  --coverage-source examples/javascript/evaluator.mjs \
  --coverage-threshold 100 \
  --coverage-command ./node_modules/.bin/c8
```

The JavaScript CLI executes datasets but does not choose an isolation backend.
For contract-scoped execution, invoke the Python isolation CLI from the Open
Contract root. It reads the contract manifest and starts this JavaScript runner
once inside the selected isolation backend:

```sh
cd ../..
python3 bin/open_contract.py run \
  --contract-dir contract-languages/javascript/examples/javascript
```

## JavaScript coverage

Coverage is collected by `c8` from the Node processes started for command cases.
The runner creates a fresh V8 coverage directory for each invocation and
enforces line, function, and branch thresholds against the selected sources.
Dataset cases are the only processes that contribute coverage. A threshold of
100 is the default when coverage sources are selected without an explicit
threshold.

```sh
node src/cli.js run examples/javascript/dataset.yaml \
  --coverage-source examples/javascript/evaluator.mjs \
  --coverage-threshold 100 \
  --coverage-command ./node_modules/.bin/c8
```

Use `--coverage-source` once per file or directory to include. The command is
resolved from `PATH` or from the supplied path; `c8` must be installed in the
runner package or otherwise available. Coverage failures make the runner exit
with status 1, just like failed contract cases.

When a measured source is outside the current working directory, add
`--coverage-allow-external`; this passes c8's external-source option through to
the report step. The source must still be selected explicitly with
`--coverage-source`.

When coverage is enabled, the normal JSON CLI report includes `coverage` on
each case and a top-level aggregate `coverage` record. Each record contains
`files` (`path` plus sorted covered `lines`) and a 32-character lowercase
hexadecimal fingerprint containing the first 128 bits of SHA-256. Paths are
relative to the common root of the selected coverage sources. The digest is
SHA-256 of UTF-8 `JSON.stringify(files)`; file
paths and line numbers are sorted before serialization, so the same file and
line set produces the same fingerprint regardless of the caller's working
directory. Sequence cases also include
`coverage.steps`, keyed by step ID. A custom executor's parallel group shares
one simultaneous coverage snapshot, so its step records say
`attribution: "parallel_group"` and carry the group's union of covered lines.
The case record contains the union across its steps; the top-level record
contains the union across the entire dataset, including custom executor setup.
Use the default detailed output to inspect these fields; `--quiet` keeps only
the pass/fail summary.

Datasets may optionally pin an observed fingerprint on a case or sequence step
with `coverage_fingerprint`. When coverage is enabled, the runner reports
`expected_fingerprint` and `baseline_matches` beside the observed fingerprint.
A mismatch fails that case and makes the CLI exit with status 1. The runner
never edits the dataset; update the value deliberately after reviewing a
coverage change. A dataset containing fingerprints must be run with coverage
collection enabled.

## Ordered sequences

A case may contain `steps` instead of a case-level matcher. Each step has an
`id` and an `input`; the runner sends that input to the same target executor,
records the returned value, and checks it immediately with its own `expected`
and/or `predicate`. Unmatched steps can set up state for later operations. A
sequence has no final matcher: checks belong beside the operation they describe.

A step's `capture` maps names to JSON Pointers in its result. Later inputs and
expected values can use `{"$capture": "name"}` to reuse a captured value or
`{"$input": "/path"}` to select from the case input. References substitute
JSON values, so numbers, booleans, arrays, and objects keep their types. A step
predicate sees the current result at its root and earlier outputs and captures
under `.sequence.steps` and `.sequence.captures`; this supports a focused
cross-step check without a separate case-level predicate.

Use a `{parallel: [...]}` item when steps are independent and may run
concurrently. Values captured by a parallel group become available after the
whole group finishes. The runner owns ordering, captures, matching, and the
sequence report; each executor defines what its input means. See the shared
[`sequences.yaml`](../examples/sequences.yaml) example.

For `custom` targets, the supplied executor function is called once per case
input or once per sequence step input. Built-in `command` and `hooks` targets
start a new process for each case or step; `http` targets send a separate
request for each. A custom in-process executor can preserve state across its
calls. On sequence calls it also receives `{case_id, step_id, step_index,
step_count, captures}` as a second argument, which lets an executor isolate
scenario state and know when its sequence has ended. Command-based sequences
need their own persistence mechanism.
