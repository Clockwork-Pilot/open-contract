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

test('executes generic ordered and parallel steps, checks outcomes, and passes typed captures forward', async () => {
	const bundle = await readBundle(new URL('../../examples/sequences.yaml', import.meta.url));
	const calls = [];
	const contexts = [];
	const report = await runBundle(bundle, { executor: async (input, context) => {
		calls.push(input);
		contexts.push(context);
		if (input.operation === 'create') return { id: input.id };
		if (input.operation === 'read') return { id: input.id, found: true };
		if (input.operation === 'status') return { active: true };
		if (input.operation === 'metadata') return { id: input.id, label: 'sample' };
		throw new Error(`unexpected operation: ${input.operation}`);
	} });
	assert.equal(report.passed, 1);
	assert.deepEqual(calls.slice(0, 2), [
		{ operation: 'create', id: 17 },
		{ operation: 'read', id: 17 },
	]);
	assert.deepEqual(calls.slice(2).map(({ operation }) => operation).sort(), ['metadata', 'status']);
	assert.deepEqual(contexts.slice(0, 2).map(({ case_id, step_id, step_index }) => ({ case_id, step_id, step_index })), [
		{ case_id: 'create-read-and-inspect', step_id: 'create', step_index: 0 },
		{ case_id: 'create-read-and-inspect', step_id: 'read', step_index: 1 },
	]);
	assert.deepEqual({ ...contexts[1].captures }, { saved_id: 17 });
	assert.deepEqual({ ...report.targets.sequences.cases[0].actual.captures }, { saved_id: 17 });
	assert.deepEqual(report.targets.sequences.cases[0].actual.checks, [
		{ id: 'create', passed: true, matched_by: 'exact' },
		{ id: 'read', passed: true, matched_by: 'predicate' },
		{ id: 'status', passed: true, matched_by: 'exact' },
		{ id: 'metadata', passed: true, matched_by: 'exact' },
	]);
});
