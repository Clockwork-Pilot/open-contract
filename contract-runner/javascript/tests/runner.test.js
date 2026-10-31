import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { readBundle } from '../src/cli.js';
import { runBundle } from '../src/runner.js';

test('reads and validates native YAML bundles and runs expected and predicate cases', async () => {
	const bundle = await readBundle(new URL('../examples/javascript/dataset.yaml', import.meta.url));
	const report = await runBundle(bundle);
	assert.equal(report.passed, 4);
	assert.equal(report.total, 4);
	assert.deepEqual(report.targets.example.cases.map(({ matched_by }) => matched_by), ['exact', 'exact', 'predicate', 'predicate']);
});

test('reads native JSON bundle form too', async () => {
	const source = await readFile(new URL('../examples/javascript/dataset.yaml', import.meta.url), 'utf8');
	const { parse } = await import('yaml');
	const directory = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-test-'));
	const filename = path.join(directory, 'dataset.json');
	try {
		const { writeFile } = await import('node:fs/promises');
		await writeFile(filename, JSON.stringify(parse(source)));
		const report = await runBundle(await readBundle(filename));
		assert.equal(report.passed, 4);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test('supports the same optional reader and custom executor adapters as Python', async () => {
	const adapted = await readBundle(new URL('../examples/javascript/dataset.yaml', import.meta.url), {
		reader: () => ({
			version: 1,
			cases: [{ id: 'adapted', input: { value: 4 }, expected: { doubled: 8 } }],
		}),
	});
	const report = await runBundle(adapted, { executor: (input) => ({ doubled: input.value * 2 }) });
	assert.equal(report.passed, 1);
});
