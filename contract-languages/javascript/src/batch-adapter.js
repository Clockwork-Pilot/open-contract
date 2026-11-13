#!/usr/bin/env node
import inspector from 'node:inspector';
import { readFile, readdir, realpath, stat } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const v8ToIstanbul = require('v8-to-istanbul');

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
const coverageSnapshots = [];
const additionalCoverageSnapshots = [];
const coverageScopes = new Map();
const requests = request.requests ?? [];
const supplementalTests = request.supplemental_tests ?? [];
const packageRoot = path.resolve(root, request.package ?? '');
let additionalTestCoverage = null;
try {
	if (packageRoot !== root && !packageRoot.startsWith(`${root}${path.sep}`)) {
		throw new Error('contract package path escaped staged root');
	}
	await post('Profiler.enable');
	await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
	const loaded = await import(pathToFileURL(validator).href);
	const validate = loaded.validate ?? loaded.default;
	if (typeof validate !== 'function') throw new Error('validator must export validate(input) or a default function');
	const setupCoverage = await post('Profiler.takePreciseCoverage');
	coverageSnapshots.push(setupCoverage);
	for (const item of requests) {
		try {
			const output = await validate(item.input);
			const serialized = JSON.stringify(output);
			if (serialized === undefined) throw new TypeError('validator output is not JSON serializable');
			results.push({ key: item.key, output: JSON.parse(serialized) });
		} catch (error) {
			results.push({ key: item.key, error: `${error?.name ?? 'Error'}: ${error?.message ?? error}` });
		}
		const snapshot = await post('Profiler.takePreciseCoverage');
		coverageSnapshots.push(snapshot);
		coverageScopes.set(JSON.stringify(item.key), {
			key: item.key,
			files: await lineCoverage(
				[setupCoverage, snapshot], root, packageRoot, request.coverage_sources ?? [],
			),
		});
	}
	if (supplementalTests.length) {
		const datasetCoverageLines = await normalizedLineCoverage(
			coverageSnapshots, packageRoot, request.coverage_sources ?? [],
		);
		const datasetPercent = coveragePercent(datasetCoverageLines);
		if (datasetPercent < 90) {
			additionalTestCoverage = {
				status: 'skipped', executed: false, passed: true,
				reason: `dataset-only line coverage ${datasetPercent.toFixed(1)}% is below the 90% threshold`,
				test_files: supplementalTests,
			};
		} else {
			const errors = [];
			let passedCount = 0;
			for (const entry of supplementalTests) {
				const testFile = path.resolve(root, entry);
				let canonicalTestFile;
				try { canonicalTestFile = await realpath(testFile); } catch (error) {
					errors.push(`${entry}: ${error?.name ?? 'Error'}: ${error?.message ?? error}`);
					continue;
				}
				if (!canonicalTestFile.startsWith(`${packageRoot}${path.sep}`) || !/\.(?:js|mjs|cjs)$/.test(canonicalTestFile)) {
					errors.push(`${entry}: supplemental test path escaped contract package`);
					continue;
				}
				try {
					await post('Profiler.takePreciseCoverage');
					const loadedTest = await import(pathToFileURL(canonicalTestFile).href);
					if (typeof loadedTest.run !== 'function') {
						throw new TypeError('supplemental test module must export async or sync run()');
					}
					const outcome = await loadedTest.run();
					if (outcome === false) throw new Error('run() returned false');
					passedCount += 1;
				} catch (error) {
					errors.push(`${entry}: ${error?.name ?? 'Error'}: ${error?.message ?? error}`);
				} finally {
					additionalCoverageSnapshots.push(await post('Profiler.takePreciseCoverage'));
				}
			}
			const passed = errors.length === 0 && passedCount === supplementalTests.length;
			additionalTestCoverage = {
				status: passed ? 'passed' : 'failed',
				executed: true,
				passed,
				test_count: passedCount,
				test_files: supplementalTests,
				error: errors.length ? errors.join('; ') : null,
				native_report: {
					format: 'v8-inspector',
					sources: [...new Set(additionalCoverageSnapshots.flatMap((snapshot) =>
						(snapshot.result ?? []).map((item) => item.url)))].sort(),
				},
			};
		}
	}
} catch (error) {
	for (const item of requests) {
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
const files = coverageSnapshots.flatMap((snapshot) => (snapshot.result ?? []).flatMap((item) => {
	if (!item.url.startsWith('file:')) return [];
	const source = fileURLToPath(item.url);
	if (source !== root && !source.startsWith(`${root}${path.sep}`)) return [];
	return [{ ...item, url: path.relative(root, source).split(path.sep).join('/') }];
}));
const coverage = {
	format: 'v8-inspector',
	sources: [...new Set(files.map((item) => item.url))].sort(),
	result: files,
};
const scopes = requests.map((item) => coverageScopes.get(JSON.stringify(item.key)) ?? ({ key: item.key, files: [] }));
const coverageLines = await normalizedLineCoverage(
	coverageSnapshots, packageRoot, request.coverage_sources ?? [],
);
const additionalCoverageLines = supplementalTests.length && additionalTestCoverage?.executed
	? await normalizedLineCoverage(additionalCoverageSnapshots, packageRoot, request.coverage_sources ?? [])
	: [];
originalStdoutWrite(`${JSON.stringify({
	results,
	coverage,
	coverage_scopes: scopes,
	coverage_lines: coverageLines,
	additional_test_coverage: additionalTestCoverage,
	additional_coverage_lines: additionalCoverageLines,
})}\n`);

async function normalizedLineCoverage(snapshots, sourceRoot, declaredSources) {
	const profiles = new Map();
	for (const snapshot of snapshots) {
		for (const script of snapshot.result ?? []) {
			if (!script.url?.startsWith('file:')) continue;
			const filename = path.resolve(fileURLToPath(script.url));
			if (filename !== sourceRoot && !filename.startsWith(`${sourceRoot}${path.sep}`)) continue;
			const relative = path.relative(sourceRoot, filename).split(path.sep).join('/');
			if (!declaredSources.some((entry) => relative === entry || relative.startsWith(`${entry.replace(/\/$/, '')}/`))) continue;
			const profile = profiles.get(filename) ?? { functions: [] };
			profile.functions.push(...(script.functions ?? []));
			profiles.set(filename, profile);
		}
	}
	const files = [];
	for (const filename of await declaredSourceFiles(sourceRoot, declaredSources)) {
		const profile = profiles.get(filename) ?? { functions: [] };
		const converter = v8ToIstanbul(filename);
		await converter.load();
		converter.applyCoverage(profile.functions);
		const converted = converter.toIstanbul();
		const record = converted[filename] ?? Object.values(converted)[0];
		if (!record) continue;
		const executable = new Set();
		const covered = new Set();
		for (const [id, location] of Object.entries(record.statementMap ?? {})) {
			const line = Number(location.start?.line);
			if (!Number.isInteger(line)) continue;
			executable.add(line);
			if (Number(record.s?.[id] ?? 0) > 0) covered.add(line);
		}
		const relative = path.relative(sourceRoot, filename).split(path.sep).join('/');
		files.push({
			path: relative,
			executable_lines: [...executable].sort((a, b) => a - b),
			covered_lines: [...covered].sort((a, b) => a - b),
		});
	}
	return files.sort((left, right) => left.path.localeCompare(right.path));
}

async function declaredSourceFiles(sourceRoot, declaredSources) {
	const files = new Set();
	const visit = async (filename) => {
		const details = await stat(filename);
		if (details.isDirectory()) {
			const entries = await readdir(filename, { withFileTypes: true });
			for (const entry of entries) {
				if (entry.isDirectory()) await visit(path.join(filename, entry.name));
				else if (entry.isFile() && /\.(?:js|mjs|cjs)$/.test(entry.name)) files.add(path.join(filename, entry.name));
			}
		} else if (/\.(?:js|mjs|cjs)$/.test(filename)) files.add(filename);
	};
	for (const entry of declaredSources) {
		const unresolved = path.resolve(sourceRoot, entry);
		const candidate = await realpath(unresolved);
		if (candidate !== sourceRoot && !candidate.startsWith(`${sourceRoot}${path.sep}`)) {
			throw new Error(`declared coverage source escaped contract package: ${entry}`);
		}
		await visit(candidate);
	}
	return [...files].sort();
}

function coveragePercent(files) {
	const executable = files.reduce((total, file) => total + file.executable_lines.length, 0);
	const covered = files.reduce((total, file) => total + file.covered_lines.length, 0);
	return executable ? covered * 100 / executable : 0;
}

async function lineCoverage(snapshots, stagedRoot, sourceRoot, declaredSources) {
	const linesByPath = new Map();
	for (const snapshot of snapshots) {
		for (const script of snapshot.result ?? []) {
			if (!script.url?.startsWith('file:')) continue;
			const filename = path.resolve(fileURLToPath(script.url));
			if (filename !== sourceRoot && !filename.startsWith(`${sourceRoot}${path.sep}`)) continue;
			const relative = path.relative(sourceRoot, filename).split(path.sep).join('/');
			if (!declaredSources.some((entry) => relative === entry || relative.startsWith(`${entry.replace(/\/$/, '')}/`))) continue;
			let source;
			try { source = await readFile(filename, 'utf8'); } catch { continue; }
			const starts = lineStarts(source);
			const lines = linesByPath.get(relative) ?? new Set();
			for (const fn of script.functions ?? []) {
				const ranges = fn.ranges ?? [];
				const boundaries = [...new Set(ranges.flatMap(({ startOffset, endOffset }) => [startOffset, endOffset]))]
					.sort((left, right) => left - right);
				for (let index = 0; index + 1 < boundaries.length; index += 1) {
					const start = boundaries[index];
					const end = boundaries[index + 1];
					const innermost = ranges.filter((range) => range.startOffset <= start && range.endOffset >= end)
						.sort((left, right) => (left.endOffset - left.startOffset) - (right.endOffset - right.startOffset))[0];
					if (innermost?.count > 0) addOverlappingLines(lines, starts, source.length, start, end);
				}
			}
			linesByPath.set(relative, lines);
		}
	}
	return [...linesByPath]
		.filter(([, lines]) => lines.size)
		.sort(([left], [right]) => left.localeCompare(right))
		.map(([source, lines]) => ({ path: source, lines: [...lines].sort((left, right) => left - right) }));
}

function lineStarts(source) {
	const starts = [0];
	for (let index = 0; index < source.length; index += 1) if (source[index] === '\n') starts.push(index + 1);
	return starts;
}

function addOverlappingLines(lines, starts, sourceLength, rangeStart, rangeEnd) {
	let index = 0;
	while (index + 1 < starts.length && starts[index + 1] <= rangeStart) index += 1;
	while (index < starts.length && starts[index] < rangeEnd) {
		const lineStart = starts[index];
		const lineEnd = starts[index + 1] ?? sourceLength;
		if (rangeStart < lineEnd && rangeEnd > lineStart) lines.add(index + 1);
		index += 1;
	}
}
