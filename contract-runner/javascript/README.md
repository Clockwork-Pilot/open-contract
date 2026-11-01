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
