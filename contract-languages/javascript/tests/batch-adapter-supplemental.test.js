import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const languagePath = fileURLToPath(new URL('../src', import.meta.url));

async function runAdapter(root, supplementalTests) {
	const request = {
		validator: 'validator.mjs',
		package: '.',
		coverage_sources: ['validator.mjs'],
		supplemental_tests: supplementalTests,
		requests: [{ key: { case: 'one' }, input: 3 }],
	};
    return spawnSync(process.execPath, [path.join(root, 'run-contract.mjs')], {
      cwd: root,
      input: JSON.stringify(request),
      encoding: 'utf8',
      env: { ...process.env, OPEN_CONTRACT_LANGUAGE_PATH: languagePath },
    });
}

async function fixture(t, validatorSource = 'export function validate(value) { return { value }; }\n') {
	const root = await mkdtemp(path.join(os.tmpdir(), 'open-contract-adapter-'));
	t.after(() => rm(root, { recursive: true, force: true }));
	await writeFile(path.join(root, 'package.json'), '{"type":"module"}');
	await writeFile(path.join(root, 'validator.mjs'), validatorSource);
	await writeFile(path.join(root, 'run-contract.mjs'), `
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const { runDataset } = await import(pathToFileURL(path.join(process.env.OPEN_CONTRACT_LANGUAGE_PATH, 'batch-adapter.js')).href);
let text = '';
for await (const chunk of process.stdin) text += chunk;
const report = await runDataset(JSON.parse(text), () => import('./validator.mjs'));
process.stdout.write(JSON.stringify(report));
`);
	return root;
}

test('reports supplemental coverage separately from dataset coverage', async (t) => {
	const root = await fixture(t);
	await writeFile(path.join(root, 'supplemental.mjs'),
		"import { validate } from './validator.mjs';\nexport function run() { if (validate(4).value !== 4) throw new Error('unexpected output'); }\n");
	const result = await runAdapter(root, ['supplemental.mjs']);
	assert.equal(result.status, 0, result.stderr);
	const report = JSON.parse(result.stdout);
	assert.equal(report.additional_test_coverage.status, 'passed');
	assert.equal(report.additional_test_coverage.executed, true);
	assert.deepEqual(report.coverage_lines, report.additional_coverage_lines);
	assert.equal(report.coverage_lines[0].path, 'validator.mjs');
});

test('reports supplemental failures without replacing dataset results', async (t) => {
	const root = await fixture(t);
	await writeFile(path.join(root, 'supplemental.mjs'), "export function run() { throw new Error('deliberate failure'); }\n");
	const result = await runAdapter(root, ['supplemental.mjs']);
	assert.equal(result.status, 0, result.stderr);
	const report = JSON.parse(result.stdout);
	assert.deepEqual(report.results, [{ key: { case: 'one' }, output: { value: 3 } }]);
	assert.equal(report.additional_test_coverage.status, 'failed');
	assert.match(report.additional_test_coverage.error, /deliberate failure/);
});

test('skips supplemental tests when dataset coverage is below 90 percent', async (t) => {
	const root = await fixture(t, 'export function validate(value) {\n if (value) {\n  return { value };\n }\n return { empty: true };\n}\n');
	await writeFile(path.join(root, 'supplemental.mjs'), "export function run() { throw new Error('must remain unexecuted'); }\n");
	const result = await runAdapter(root, ['supplemental.mjs']);
	assert.equal(result.status, 0, result.stderr);
	const report = JSON.parse(result.stdout);
	assert.equal(report.additional_test_coverage.status, 'skipped');
	assert.equal(report.additional_test_coverage.executed, false);
	assert.match(report.additional_test_coverage.reason, /below the 90% threshold/);
});
