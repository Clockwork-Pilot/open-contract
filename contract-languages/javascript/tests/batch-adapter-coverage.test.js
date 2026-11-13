import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { runDataset } from '../src/batch-adapter.js';

test('dataset coverage is the union of normalized per-case scopes', async () => {
	const packageRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-coverage-'));
	const originalDirectory = process.cwd();
	const validatorPath = path.join(packageRoot, 'validator.mjs');
	await writeFile(path.join(packageRoot, 'package.json'), '{"type":"module"}');
	await writeFile(validatorPath, [
		'export function validate(input) {',
		'\tif (input.mode === "yes") {',
		'\t\treturn { value: "yes" };',
		'\t}',
		'\treturn { value: "no" };',
		'}',
	].join('\n'));
	process.chdir(packageRoot);
	try {
		const report = await runDataset({
			package: '.',
			validator: 'validator.mjs',
			coverage_sources: ['validator.mjs'],
			requests: [
				{ key: { target_id: 'target', case_id: 'yes' }, input: { mode: 'yes' } },
				{ key: { target_id: 'target', case_id: 'no' }, input: { mode: 'no' } },
			],
		}, () => import(pathToFileURL(validatorPath).href));
		const scopedLines = new Set(report.coverage_scopes.flatMap((scope) =>
			scope.files.flatMap((file) => file.lines)));
		assert.deepEqual(report.coverage_lines[0].covered_lines, [...scopedLines].sort((a, b) => a - b));
		assert.equal(report.coverage_lines[0].path, 'validator.mjs');
		assert.equal(report.results.length, 2);
	} finally {
		process.chdir(originalDirectory);
		await rm(packageRoot, { recursive: true, force: true });
	}
});
