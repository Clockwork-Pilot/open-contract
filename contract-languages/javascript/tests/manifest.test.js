import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import YAML from 'yaml';
import { main } from '../src/cli.js';
import { readManifest, stagePackage } from '../src/manifest.js';

async function makePackage(root, name, input) {
	await mkdir(root, { recursive: true });
	await writeFile(path.join(root, 'data.yaml'), YAML.stringify({ version: 1, cases: [{ id: 'case', input, expected: input + 1 }] }));
	await writeFile(path.join(root, 'check.mjs'), 'export function validate(value) { return value + 1; }\n');
	const manifest = {
		version: '1', name,
		files: { dataset: 'data.yaml', validator: 'check.mjs', support: [], nested_contracts: [] },
		filesystem: { read: ['data.yaml', 'check.mjs'], write: [] },
	};
	const manifestPath = path.join(root, 'contract.yaml');
	await writeFile(manifestPath, YAML.stringify(manifest));
	return manifestPath;
}

test('manifest staging copies only declared contract files', async () => {
	const temp = await mkdtemp(path.join(os.tmpdir(), 'open-contract-test-'));
	try {
		const manifestPath = await makePackage(path.join(temp, 'package'), 'sample', 3);
		await writeFile(path.join(temp, 'package', 'private.txt'), 'secret');
		const { manifest, root } = await readManifest(manifestPath);
		const staged = await stagePackage(manifest, root);
		try {
			await readFile(path.join(staged, 'data.yaml'));
			await assert.rejects(readFile(path.join(staged, 'private.txt')), { code: 'ENOENT' });
		} finally { await rm(staged, { recursive: true, force: true }); }
	} finally { await rm(temp, { recursive: true, force: true }); }
});

test('manifest parser rejects traversal', async () => {
	const temp = await mkdtemp(path.join(os.tmpdir(), 'open-contract-test-'));
	try {
		const manifestPath = await makePackage(temp, 'sample', 3);
		const manifest = YAML.parse(await readFile(manifestPath, 'utf8'));
		manifest.files.dataset = '../outside.yaml';
		await writeFile(manifestPath, YAML.stringify(manifest));
		await assert.rejects(readManifest(manifestPath), /stay within the package/);
	} finally { await rm(temp, { recursive: true, force: true }); }
});

test('parent manifest executes nested contracts in the same report', async () => {
	const temp = await mkdtemp(path.join(os.tmpdir(), 'open-contract-test-'));
	const originalLog = console.log;
	let output = '';
	console.log = (value) => { output += `${value}\n`; };
	try {
		const parentPath = await makePackage(path.join(temp, 'parent'), 'parent', 3);
		const childPath = await makePackage(path.join(temp, 'parent', 'child'), 'child', 7);
		const manifest = YAML.parse(await readFile(parentPath, 'utf8'));
		manifest.files.nested_contracts = ['child/contract.yaml'];
		manifest.filesystem.read = ['data.yaml', 'check.mjs', 'child'];
		await writeFile(parentPath, YAML.stringify(manifest));
		assert.equal(await main(['run-manifest', parentPath, '--isolation', 'landlock', '--coverage-threshold', '0', '--coverage-allow-external', '--quiet']), 0);
		assert.match(output, /Cases passed: 2\/2/);
		assert.ok(childPath);
	} finally {
		console.log = originalLog;
		await rm(temp, { recursive: true, force: true });
	}
});
