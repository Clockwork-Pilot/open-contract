import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('CLI loads optional reader and custom executor exports', async () => {
	const { stdout } = await execFileAsync(process.execPath, [
		'src/cli.js', 'run', 'examples/javascript/dataset.yaml',
		'--reader', './tests/fixtures/adapters.mjs:readLegacy',
		'--executor', './tests/fixtures/adapters.mjs:evaluate',
	], { cwd: packageRoot });
	assert.match(stdout, /"passed": 1/);
	assert.match(stdout, /"total": 1/);
});

test('CLI can enforce coverage for production sources outside the runner package', async () => {
	const { stdout, stderr } = await execFileAsync(process.execPath, [
		'src/cli.js', 'run', 'tests/fixtures/external-coverage-dataset.yaml',
		'--coverage-source', '../coverage-fixtures/evaluator.mjs',
		'--coverage-source', 'tests/fixtures/internal-coverage-helper.mjs',
		'--coverage-threshold', '100',
		'--coverage-command', './node_modules/.bin/c8',
		'--coverage-allow-external',
	], { cwd: packageRoot });
	assert.match(stdout, /"passed": 1/);
	assert.match(stderr, /JavaScript coverage threshold passed: all metrics >= 100%/);
});

test('CLI exports a standard Istanbul coverage artifact for suite aggregation', async () => {
	const directory = await mkdtemp(path.join(os.tmpdir(), 'open-contract-coverage-artifact-'));
	const artifact = path.join(directory, 'coverage-final.json');
	try {
		const { stdout, stderr } = await execFileAsync(process.execPath, [
			'src/cli.js', 'run', 'tests/fixtures/external-coverage-dataset.yaml',
			'--coverage-source', '../coverage-fixtures/evaluator.mjs',
			'--coverage-source', 'tests/fixtures/internal-coverage-helper.mjs',
			'--coverage-threshold', '100',
			'--coverage-command', './node_modules/.bin/c8',
			'--coverage-allow-external',
		], { cwd: packageRoot, env: { ...process.env, OPEN_CONTRACT_COVERAGE_ARTIFACT: artifact } });
		const coverage = JSON.parse(await readFile(artifact, 'utf8'));
		assert.ok(Object.keys(coverage).length >= 2);
		assert.ok(Object.values(coverage).every((source) => source.statementMap && source.fnMap && source.branchMap));
		assert.match(stdout, /"passed": 1/);
		assert.match(stderr, /JavaScript coverage threshold passed: all metrics >= 100%/);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test('CLI accepts coverage delegated by a nested test runner', async () => {
	const directory = await mkdtemp(path.join(os.tmpdir(), 'open-contract-delegated-coverage-'));
	const artifact = path.join(directory, 'coverage-final.json');
	const dataset = path.join(directory, 'dataset.yaml');
	const source = path.join(packageRoot, 'tests/fixtures/internal-coverage-helper.mjs');
	const caseFingerprint = createHash('sha256').update(JSON.stringify([
		{ path: 'internal-coverage-helper.mjs', lines: [2] },
	])).digest('hex').slice(0, 32);
	const delegated = {
		[source]: {
			path: source,
			statementMap: { 0: { start: { line: 2, column: 1 }, end: { line: 2, column: 14 } } },
			fnMap: {}, branchMap: {}, s: { 0: 1 }, f: {}, b: {},
		},
	};
	try {
		await writeFile(dataset, [
			'version: 1', 'targets:', '  - id: delegated', '    kind: custom', '    cases:',
			'      - id: one', `        coverage_fingerprint: ${caseFingerprint}`,
			'        input: {branch: alpha}', '        expected: {branch: alpha}',
		].join('\n'));
		await writeFile(`${artifact}.delegated.json`, JSON.stringify(delegated));
		await writeFile(`${artifact}.scopes.json`, JSON.stringify({ 'delegated/one': delegated }));
		const { stdout, stderr } = await execFileAsync(process.execPath, [
			'src/cli.js', 'run', dataset,
			'--executor', './coverage-fixtures/coverage-executor.mjs:evaluate',
			'--coverage-source', source,
			'--coverage-threshold', '100',
			'--coverage-command', './node_modules/.bin/c8',
		], { cwd: packageRoot, env: { ...process.env, OPEN_CONTRACT_COVERAGE_ARTIFACT: artifact } });
		assert.match(stdout, /"passed": 1/);
		assert.match(stderr, /JavaScript coverage threshold passed: all metrics >= 100%/);
		assert.deepEqual(JSON.parse(await readFile(artifact, 'utf8')), delegated);
		const report = JSON.parse(stdout);
		assert.deepEqual(report.targets.delegated.cases[0].coverage.files, [
			{ path: 'internal-coverage-helper.mjs', lines: [2] },
		]);
		assert.equal(report.targets.delegated.cases[0].coverage.baseline_matches, true);
		assert.deepEqual(report.coverage.files, report.targets.delegated.cases[0].coverage.files);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test('CLI reports deterministic per-case and per-step covered lines with fingerprints', async () => {
	const { stdout, stderr } = await execFileAsync(process.execPath, [
		'src/cli.js', 'run', 'tests/fixtures/coverage-sequences.yaml',
		'--executor', './coverage-fixtures/coverage-executor.mjs:evaluate',
		'--coverage-source', 'coverage-fixtures/coverage-executor.mjs',
		'--coverage-threshold', '100',
		'--coverage-command', './node_modules/.bin/c8',
	], { cwd: packageRoot, maxBuffer: 8 * 1024 * 1024 });
	const report = JSON.parse(stdout);
	const cases = report.targets.coverage.cases;
	assert.equal(report.passed, 3);
	assert.match(stderr, /JavaScript coverage threshold passed: all metrics >= 100%/);
	assert.deepEqual(cases[0].coverage.files, cases[1].coverage.files);
	assert.equal(cases[0].coverage.fingerprint, cases[1].coverage.fingerprint);
	assert.equal(cases[0].coverage.baseline_matches, true);
	assert.equal(cases[2].coverage.steps.alpha.fingerprint, cases[0].coverage.fingerprint);
	assert.equal(cases[2].coverage.steps.alpha.baseline_matches, true);
	assert.notEqual(cases[2].coverage.steps.alpha.fingerprint, cases[2].coverage.steps.beta.fingerprint);
	assert.ok(cases[2].coverage.steps.alpha.files[0].lines.length > 0);
	assert.ok(report.coverage.files[0].lines.length > 0);
	for (const entry of [cases[0].coverage, cases[2].coverage.steps.alpha, cases[2].coverage.steps.beta, report.coverage]) {
		assert.match(entry.fingerprint, /^[a-f0-9]{32}$/);
	}
});

test('CLI fails when a stored case coverage fingerprint no longer matches', async () => {
	const original = await readFile(path.join(packageRoot, 'tests/fixtures/coverage-sequences.yaml'), 'utf8');
	const fixture = await mkdtemp(path.join(os.tmpdir(), 'open-contract-baseline-'));
	const dataset = path.join(fixture, 'dataset.yaml');
	try {
		await writeFile(dataset, original.replace(
			'0f5c0920f3240054cb84ba01b9c1ff10',
			'a'.repeat(32),
		));
		await assert.rejects(execFileAsync(process.execPath, [
			'src/cli.js', 'run', dataset,
			'--executor', './coverage-fixtures/coverage-executor.mjs:evaluate',
			'--coverage-source', 'coverage-fixtures/coverage-executor.mjs',
			'--coverage-threshold', '100',
			'--coverage-command', './node_modules/.bin/c8',
		], { cwd: packageRoot, maxBuffer: 8 * 1024 * 1024 }), /coverage fingerprint mismatch/);
	} finally {
		await rm(fixture, { recursive: true, force: true });
	}
});

test('CLI includes coverage for sequence steps that throw', async () => {
	let report;
	await assert.rejects(execFileAsync(process.execPath, [
		'src/cli.js', 'run', 'tests/fixtures/coverage-failure-sequence.yaml',
		'--executor', './coverage-fixtures/coverage-failure-executor.mjs:evaluate',
		'--coverage-source', 'coverage-fixtures/coverage-failure-executor.mjs',
		'--coverage-threshold', '100',
		'--coverage-command', './node_modules/.bin/c8',
	], { cwd: packageRoot, maxBuffer: 8 * 1024 * 1024 }), (error) => {
		report = JSON.parse(error.stdout);
		assert.match(error.stderr, /JavaScript coverage threshold passed: all metrics >= 100%/);
		return report.passed === 2 && report.total === 3;
	});
	const failed = report.targets.coverage.cases[2];
	assert.equal(failed.passed, false);
	assert.equal(failed.coverage.steps.completed.fingerprint, report.targets.coverage.cases[0].coverage.fingerprint);
	assert.ok(failed.coverage.steps.throws.files[0].lines.length > 0);
});

function execFileAsync(command, args, options) {
	return new Promise((resolve, reject) => execFile(command, args, options, (error, stdout, stderr) => {
		if (error) {
			const failure = new Error(`${stdout}\n${stderr}\n${error.message}`);
			failure.stdout = stdout;
			failure.stderr = stderr;
			reject(failure);
		}
		else resolve({ stdout, stderr });
	}));
}
