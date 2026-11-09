import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const repo = resolve(import.meta.dirname, '..');
const launcher = resolve(repo, 'bin/landlock-run');
const python = process.env.PYTHON ?? 'python3';
const abiProbe = spawnSync(python, ['-c', 'import ctypes; print(ctypes.CDLL(None).syscall(444, 0, 0, 1))'], { encoding: 'utf8' });
const abi = Number(abiProbe.stdout.trim());
const landlockAvailable = process.platform === 'linux' && abiProbe.status === 0 && abi >= 1 && abi <= 4;
const systemReads = ['/usr', '/bin', '/lib', '/lib64'].filter((path) => {
	return existsSync(path);
});

test('Landlock allows declared reads and writes while denying undeclared paths', { skip: !landlockAvailable }, () => {
	const root = mkdtempSync(resolve(tmpdir(), 'open-contract-landlock-test-'));
	const allowed = resolve(root, 'allowed');
	const denied = resolve(root, 'denied');
	const input = resolve(allowed, 'input.txt');
	const outside = resolve(denied, 'input.txt');
	const output = resolve(allowed, 'output.txt');
	try {
		mkdirSync(allowed);
		mkdirSync(denied);
		writeFileSync(input, 'permitted');
		writeFileSync(outside, 'forbidden');
		const args = [launcher];
		for (const path of systemReads) args.push('--read', path);
		args.push('--read', allowed, '--write', allowed, '--', '/bin/sh', '-c',
			'read line < "$1"; printf "%s" "$line"; printf saved > "$2"; read line < "$3"',
			'test-shell', input, output, outside);
		const result = spawnSync(python, args, { encoding: 'utf8' });
		assert.equal(result.status, 2, result.stderr);
		assert.equal(result.stdout, 'permitted');
		assert.equal(readFileSync(output, 'utf8'), 'saved');
		assert.match(result.stderr, /Permission denied/);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test('Landlock fails closed for an invalid allowlist before starting the child', { skip: !landlockAvailable }, () => {
	const root = mkdtempSync(resolve(tmpdir(), 'open-contract-landlock-invalid-'));
	try {
		const missing = resolve(root, 'missing');
		const result = spawnSync(python, [launcher, '--read', missing, '--', '/bin/sh', '-c', 'touch "$1"', 'test-shell', resolve(root, 'started')], { encoding: 'utf8' });
		assert.equal(result.status, 1);
		assert.match(result.stderr, /allowed path does not exist/);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
