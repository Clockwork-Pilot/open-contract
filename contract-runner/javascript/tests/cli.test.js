import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
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

function execFileAsync(command, args, options) {
	return new Promise((resolve, reject) => execFile(command, args, options, (error, stdout, stderr) => {
		if (error) reject(new Error(stderr || error.message));
		else resolve({ stdout, stderr });
	}));
}
