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
