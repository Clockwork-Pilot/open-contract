# JavaScript contract-language support

This package provides an importable batch interface for a contract-owned
JavaScript entrypoint. It is not a runner: it does not parse manifests, create
isolation, compare expected values, or evaluate predicates.

A contract command imports `runDataset`, supplies a loader for its own
`validate(input)` function, and returns the resulting JSON object to the Rust
runner. The adapter processes the flattened requests in order, resolves captures
from earlier steps, returns one keyed result per request, and reports JavaScript
coverage. Rust verifies expected values and predicates after the command exits.

The language adapter can use metadata in the existing target `config` object.
For JavaScript contracts, step metadata may be stored at
`javascript.cases.<case-id>.steps.<step-id>`. A `capture` object maps capture names
to JSON Pointers. A contiguous run of `parallel: true` steps for the same case
is invoked concurrently. Other requests execute in order. Results retain input
order, and captures are resolved between batches rather than between concurrent
steps.

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

Run the interface tests with `make -C contract-languages/javascript test` from
the `open-contract` directory. They exercise the importable adapter protocol
and do not contribute to contract dataset coverage.

Run the end-to-end contract example with `make check-javascript-example` from
the `open-contract` directory. It installs the JavaScript adapter dependencies,
then runs a complete contract manifest through the Rust runner.
