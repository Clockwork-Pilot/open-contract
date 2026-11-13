# JavaScript contract-language support

This package provides an importable batch interface for a contract-owned
JavaScript entrypoint. It is not a runner: it does not parse manifests, create
isolation, compare expected values, or evaluate predicates.

A contract command imports `runDataset`, supplies a loader for its own
`validate(input)` function, and returns the resulting JSON object to the Rust
runner. `runDataset` iterates the flattened batch once, preserves each
contract/case/step key, captures validator errors as results, and reports
JavaScript coverage. Rust verifies expected values and predicates after
commands finish.

```js
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const languagePath = process.env.OPEN_CONTRACT_LANGUAGE_PATH;
const { runDataset } = await import(
  pathToFileURL(path.join(languagePath, 'batch-adapter.js')).href
);
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const report = await runDataset(request, () => import('./validator.mjs'));
process.stdout.write(JSON.stringify(report));
```

The manifest carries only the logical `language_package` identifier and the
contract's `execution.command`. Runner configuration maps that identifier to
the support package root; the runner provides its isolated location through
`OPEN_CONTRACT_LANGUAGE_PATH`. No support file list or host path belongs in the
manifest.

Run the interface tests with `npm test` from this directory. They exercise the
importable adapter protocol and do not contribute to contract dataset coverage.
