#!/usr/bin/env node
import inspector from 'node:inspector';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';

const inputText = await new Promise((resolve, reject) => {
	let text = '';
	process.stdin.setEncoding('utf8');
	process.stdin.on('data', (chunk) => { text += chunk; });
	process.stdin.on('end', () => resolve(text));
	process.stdin.on('error', reject);
});

const request = JSON.parse(inputText);
const root = process.cwd();
const validator = path.resolve(root, request.validator);
if (validator !== root && !validator.startsWith(`${root}${path.sep}`)) {
	throw new Error('validator path escaped staged package root');
}

const originalStdoutWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, ...args) => process.stderr.write(chunk, ...args);
console.log = (...args) => console.error(...args);

const session = new inspector.Session();
session.connect();
const post = (method, params = {}) => new Promise((resolve, reject) => {
	session.post(method, params, (error, result) => error ? reject(error) : resolve(result));
});
const results = [];
let coverage = null;
try {
	await post('Profiler.enable');
	await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
	const loaded = await import(pathToFileURL(validator).href);
	const validate = loaded.validate ?? loaded.default;
	if (typeof validate !== 'function') throw new Error('validator must export validate(input) or a default function');
	for (const item of request.requests ?? []) {
		try {
			const output = await validate(item.input);
			const serialized = JSON.stringify(output);
			if (serialized === undefined) throw new TypeError('validator output is not JSON serializable');
			results.push({ key: item.key, output: JSON.parse(serialized) });
		} catch (error) {
			results.push({ key: item.key, error: `${error?.name ?? 'Error'}: ${error?.message ?? error}` });
		}
	}
	const nativeCoverage = await post('Profiler.takePreciseCoverage');
	const files = (nativeCoverage.result ?? []).flatMap((item) => {
		if (!item.url.startsWith('file:')) return [];
		const source = fileURLToPath(item.url);
		if (source !== root && !source.startsWith(`${root}${path.sep}`)) return [];
		const relative = path.relative(root, source).split(path.sep).join('/');
		return [{ ...item, url: relative }];
	});
	coverage = {
		...nativeCoverage,
		format: 'v8-inspector',
		sources: [...new Set(files.map((item) => item.url))].sort(),
		result: files,
	};
} catch (error) {
	for (const item of request.requests ?? []) {
		if (!results.some((result) => JSON.stringify(result.key) === JSON.stringify(item.key))) {
			results.push({ key: item.key, error: `${error?.name ?? 'Error'}: ${error?.message ?? error}` });
		}
	}
} finally {
	try { await post('Profiler.stopPreciseCoverage'); } catch {}
	try { await post('Profiler.disable'); } catch {}
	session.disconnect();
}

process.stdout.write = originalStdoutWrite;
originalStdoutWrite(`${JSON.stringify({ results, coverage })}\n`);
