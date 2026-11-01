# Open Contracts JavaScript runner

The JavaScript runner reads native Open Contract v1 JSON and YAML bundles. It
validates bundles with the shared schemas, runs built-in `command`, `hooks`,
and `http` targets, and matches either exact JSON values or the shared predicate
language. No project-specific reader is required for native Open Contract bundles.

For compatibility with the Python runner, optional `--reader MODULE:CALLABLE`
adapts a parsed source document to an Open Contract bundle, and
`--executor MODULE:CALLABLE` handles targets whose kind is `custom`. Both
exports may be async JavaScript functions. Native v1 bundles and built-in
targets need neither option. Adapter paths resolve from the current working
directory; for example, `--executor ./evaluator.mjs:evaluate`.

From the Open Contract repository root, run the example with:

```sh
cd contract-runner/javascript
npm install
npm test
node src/cli.js run examples/javascript/dataset.yaml
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

## Ordered sequences

A case may contain `steps` as well as its usual `input` and final matcher. Each
step has an `id` and an `input`; the runner sends that input to the same target
executor, records the returned value under `actual.steps`, and can check the
step with its own `expected` or `predicate`. A step's `capture` maps names to
JSON Pointers in its result. Later inputs can use `{"$capture": "name"}` to
reuse a captured value or `{"$input": "/path"}` to select from the case input.
References substitute JSON values, so numbers, booleans, arrays, and objects
keep their types.

Use a `{parallel: [...]}` item when steps are independent and may run
concurrently. Values captured by a parallel group become available after the
whole group finishes. Step expectations contribute to the case result, and the
case's final matcher can inspect `actual.steps`, `actual.captures`, and
`actual.checks`. The runner only sequences executor calls; each executor still
defines what its input means. See the shared
[`sequences.yaml`](../examples/sequences.yaml) example.

For `custom` targets, the supplied executor function is called once per case
input or once per sequence step input. Built-in `command` and `hooks` targets
start a new process for each case or step; `http` targets send a separate
request for each. A custom in-process executor can preserve state across its
calls. On sequence calls it also receives `{case_id, step_id, step_index,
captures}` as a second argument, which lets an executor isolate scenario state.
Command-based sequences need their own persistence mechanism.
