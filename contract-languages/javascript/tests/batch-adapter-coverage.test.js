import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { runDataset } from '../src/batch-adapter.js';

test('parallel steps share one coverage scope and receive setup context', async () => {
	const packageRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-coverage-'));
	const originalDirectory = process.cwd();
	const validatorPath = path.join(packageRoot, 'validator.mjs');
	await writeFile(path.join(packageRoot, 'package.json'), '{"type":"module"}');
	await writeFile(validatorPath, [
		'export async function validate(input, context) {',
		'\tif (context.setup !== "fixture") throw new Error("setup context missing");',
		'\tawait new Promise((resolve) => setTimeout(resolve, 1));',
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
				{ key: { case_id: 'same', step_id: 'yes' }, input: { mode: 'yes' }, setup: 'fixture', parallel: true },
				{ key: { case_id: 'same', step_id: 'no' }, input: { mode: 'no' }, setup: 'fixture', parallel: true },
			],
		}, () => import(pathToFileURL(validatorPath).href));
		const scopedLines = new Set(report.coverage_scopes.flatMap((scope) =>
			scope.files.flatMap((file) => file.lines)));
		assert.deepEqual(report.coverage_lines[0].covered_lines, [...scopedLines].sort((a, b) => a - b));
		assert.equal(report.coverage_lines[0].path, 'validator.mjs');
		assert.equal(report.results.length, 2);
		assert.deepEqual(report.coverage_scopes[0].files, report.coverage_scopes[1].files);
		assert.equal(report.results[0].key.case_id, 'same');
	} finally {
		process.chdir(originalDirectory);
		await rm(packageRoot, { recursive: true, force: true });
	}
});
