# JavaScript contract-language support

This package provides an importable batch interface for a contract-owned
JavaScript entrypoint. It is not a runner: it does not parse manifests, create
isolation, compare expected values, or evaluate predicates.

A contract command imports `runDataset`, supplies a loader for its own
`validate(input, context)` function, and returns the resulting JSON object to
the Rust runner. Context contains `case_id`, optional `step_id`, optional
`setup`, and `parallel`. The adapter processes calls in order and runs adjacent
`parallel: true` steps in a case concurrently. It returns one keyed result per
request and reports JavaScript coverage. Rust verifies expected values and
predicates after the command exits.

Setup IDs are passed through as metadata for contract-owned validator setup.
The adapter does not interpret the ID or alter the call input. Every step in a
parallel group receives the same group coverage scope.

```js
import { pathToFileURL } from 'node:url';

const entrypoint = process.env.OPEN_CONTRACT_LANGUAGE_ENTRYPOINT;
if (!entrypoint) throw new Error('OPEN_CONTRACT_LANGUAGE_ENTRYPOINT is required');
const { runDataset } = await import(pathToFileURL(entrypoint).href);
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const report = await runDataset(request, () => import('./validator.mjs'));
process.stdout.write(JSON.stringify(report));
```

The contract manifest points `language_adapter` to the adapter's `runtime.yaml`.
That file declares its entrypoint, runtime files, and environment. The runner
exposes the resolved entrypoint as `OPEN_CONTRACT_LANGUAGE_ENTRYPOINT`; adapter
runtime files do not need duplicate entries in the contract's filesystem policy.

Run the interface tests with `make -C contract-languages/javascript test` from
the `open-contract` directory. They exercise the importable adapter protocol
and do not contribute to contract dataset coverage.

For declared JavaScript sources, the adapter provides line, statement, function,
and branch entities with locations and hit counts from its V8/Istanbul coverage
data. The Rust runner's `--coverage-html` option renders those entities in the
shared report.

Run the end-to-end contract example with `make check-javascript-example` from
the `open-contract` directory. It installs the JavaScript adapter dependencies,
then runs a complete contract manifest through the Rust runner.
