import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import v8 from 'node:v8';
import { flushV8Coverage, runBundle } from './runner.js';

async function loadCallable(spec) {
	const separator = spec.lastIndexOf(':');
	if (separator <= 0 || separator === spec.length - 1) throw new Error(`executor must use MODULE:CALLABLE syntax: ${spec}`);
	const moduleName = spec.slice(0, separator);
	const exportName = spec.slice(separator + 1);
	const moduleUrl = moduleName.startsWith('.') || moduleName.startsWith('/')
		? pathToFileURL(path.resolve(moduleName))
		: moduleName;
	const loaded = await import(moduleUrl);
	const callable = loaded[exportName];
	if (typeof callable !== 'function') throw new Error(`executor is not callable: ${spec}`);
	return callable;
}

const [bundlePath, executorSpec] = process.argv.slice(2);
if (!bundlePath || !executorSpec) throw new Error('coverage worker requires bundle and executor paths');
const bundle = JSON.parse(await readFile(bundlePath, 'utf8'));
const executor = await loadCallable(executorSpec);
const coverageDirectory = process.env.NODE_V8_COVERAGE;
if (coverageDirectory) await flushV8Coverage(path.join(coverageDirectory, 'setup'));
const report = await runBundle(bundle, { coverageDirectory, executor });
if (coverageDirectory) v8.stopCoverage();
process.stdout.write(JSON.stringify(report));
