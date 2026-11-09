import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const repo = resolve(import.meta.dirname, '..');
const launcher = resolve(repo, 'bin/bwrap-run');

test('Bubblewrap probe checks startup without invoking a contract command', () => {
	const root = mkdtempSync(resolve(tmpdir(), 'open-contract-bwrap-probe-'));
	const bin = resolve(root, 'bin');
	const log = resolve(root, 'bwrap.log');
	try {
		mkdirSync(bin);
		const bwrap = resolve(bin, 'bwrap');
		writeFileSync(bwrap, '#!/bin/sh\nprintf "%s\\n" "$*" > "$BWRAP_LOG"\nexit "${BWRAP_STATUS:-0}"\n');
		chmodSync(bwrap, 0o755);
		const result = spawnSync('/bin/sh', [launcher, '--probe', '--ro-bind', '/usr', '/usr'], {
			encoding: 'utf8',
			env: { ...process.env, PATH: `${bin}:/usr/bin:/bin`, BWRAP_LOG: log },
		});
		assert.equal(result.status, 0, result.stderr);
		const args = readFileSync(log, 'utf8');
		assert.match(args, /--unshare-all/);
		assert.match(args, /--ro-bind \/ \/ /);
		assert.match(args, /--ro-bind \/usr \/usr/);
		assert.match(args, /-- \/bin\/true\s*$/);
		assert.doesNotMatch(args, /contract-runner|node /);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test('Bubblewrap probe preserves startup failure and reports missing Bubblewrap', () => {
	const root = mkdtempSync(resolve(tmpdir(), 'open-contract-bwrap-probe-failure-'));
	const bin = resolve(root, 'bin');
	try {
		mkdirSync(bin);
		const bwrap = resolve(bin, 'bwrap');
		writeFileSync(bwrap, '#!/bin/sh\nexit 47\n');
		chmodSync(bwrap, 0o755);
		const failed = spawnSync('/bin/sh', [launcher, '--probe'], {
			encoding: 'utf8', env: { ...process.env, PATH: `${bin}:/usr/bin:/bin` },
		});
		assert.equal(failed.status, 47);

		const emptyBin = resolve(root, 'empty-bin');
		mkdirSync(emptyBin);
		const missing = spawnSync('/bin/sh', [launcher, '--probe'], {
			encoding: 'utf8', env: { ...process.env, PATH: emptyBin },
		});
		assert.equal(missing.status, 2);
		assert.match(missing.stderr, /bwrap is not installed or not on PATH/);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
