import { pathToFileURL } from 'node:url';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const adapterEntrypoint = process.env.OPEN_CONTRACT_LANGUAGE_ENTRYPOINT;
if (!adapterEntrypoint) throw new Error('OPEN_CONTRACT_LANGUAGE_ENTRYPOINT is required');
const { runDataset } = await import(pathToFileURL(adapterEntrypoint).href);
const report = await runDataset(request, () => import('./validator.mjs'));
process.stdout.write(JSON.stringify(report));
