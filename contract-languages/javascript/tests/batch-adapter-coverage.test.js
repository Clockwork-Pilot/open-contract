import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { runDataset } from '../src/batch-adapter.js';

test('coverage reports only explicitly declared public source files', async () => {
	const packageRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-coverage-'));
	const originalDirectory = process.cwd();
	const validatorPath = path.join(packageRoot, 'validator.mjs');
	const helperPath = path.join(packageRoot, 'helper.mjs');
	await writeFile(path.join(packageRoot, 'package.json'), '{"type":"module"}');
	await writeFile(helperPath, 'export const answer = () => 42;\n');
	await writeFile(validatorPath, [
		'import { answer } from "./helper.mjs";',
		'export function validate() { return answer(); }',
	].join('\n'));
	process.chdir(packageRoot);
	try {
		const report = await runDataset({
			package: '.',
			coverage_sources: ['validator.mjs'],
			requests: [{ key: { case_id: 'one', step_id: null }, input: {} }],
		}, () => import(pathToFileURL(validatorPath).href));
		assert.deepEqual(report.coverage.sources, ['validator.mjs']);
		assert.ok(report.coverage.result.length > 0);
		assert.ok(report.coverage.result.every((entry) => entry.url === 'validator.mjs'));
		assert.deepEqual(report.coverage_lines.map((entry) => entry.path), ['validator.mjs']);
	} finally {
		process.chdir(originalDirectory);
		await rm(packageRoot, { recursive: true, force: true });
	}
});

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
		assert.deepEqual(report.coverage_lines[0].executable_lines, [...new Set(report.coverage_lines[0].executable_lines)].sort((a, b) => a - b));
		assert.deepEqual(report.coverage_lines[0].covered_lines, [...new Set(report.coverage_lines[0].covered_lines)].sort((a, b) => a - b));
		assert.ok(report.coverage_lines[0].covered_lines.every((line) => report.coverage_lines[0].executable_lines.includes(line)));
		assert.equal(report.results.length, 2);
		assert.deepEqual(report.coverage_scopes[0].files, report.coverage_scopes[1].files);
		assert.equal(report.results[0].key.case_id, 'same');
	} finally {
		process.chdir(originalDirectory);
		await rm(packageRoot, { recursive: true, force: true });
	}
});

test('disabling fingerprints keeps aggregate dataset line coverage', async () => {
	const packageRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-coverage-'));
	const originalDirectory = process.cwd();
	const validatorPath = path.join(packageRoot, 'validator.mjs');
	await writeFile(path.join(packageRoot, 'package.json'), '{"type":"module"}');
	await writeFile(validatorPath, [
		'export function validate(input) {',
		'\tif (input.valid) return { value: "valid" };',
		'\treturn { value: "invalid" };',
		'}',
	].join('\n'));
	process.chdir(packageRoot);
	try {
		const report = await runDataset({
			package: '.',
			coverage_sources: ['validator.mjs'],
			coverage_fingerprints: false,
			requests: [
				{ key: { case_id: 'valid', step_id: null }, input: { valid: true } },
				{ key: { case_id: 'invalid', step_id: null }, input: { valid: false } },
			],
		}, () => import(pathToFileURL(validatorPath).href));
		assert.deepEqual(report.coverage_scopes, []);
		assert.equal(report.coverage_lines.length, 1);
		assert.deepEqual(report.coverage_lines[0].covered_lines, report.coverage_lines[0].executable_lines);
		assert.ok(report.coverage_lines[0].executable_lines.every((line) => Number.isInteger(line) && line > 0));
	} finally {
		process.chdir(originalDirectory);
		await rm(packageRoot, { recursive: true, force: true });
	}
});
