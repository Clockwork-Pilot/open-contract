#!/usr/bin/env node
import inspector from 'node:inspector';
import { readdir, realpath, stat } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const v8ToIstanbul = require('v8-to-istanbul');

export async function runDataset(request, validatorLoader = null) {
const root = process.cwd();
const validator = request.validator == null ? null : path.resolve(root, request.validator);
if (validator !== null && validator !== root && !validator.startsWith(`${root}${path.sep}`)) {
	throw new Error('validator path escaped staged package root');
}
if (validator === null && typeof validatorLoader !== 'function') {
	throw new Error('validatorLoader is required when request.validator is omitted');
}

const originalStdoutWrite = process.stdout.write.bind(process.stdout);
const originalConsoleLog = console.log;
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
const datasetCoverageByFile = new Map();
let coverageLines = [];
const requests = request.requests ?? [];
const supplementalTests = request.supplemental_tests ?? [];
const packageRoot = path.resolve(root, request.package ?? '');
let additionalTestCoverage = null;
let declaredSourcePaths = new Set();
try {
	if (packageRoot !== root && !packageRoot.startsWith(`${root}${path.sep}`)) {
		throw new Error('contract package path escaped staged root');
	}
	declaredSourcePaths = new Set(await declaredSourceFiles(packageRoot, request.coverage_sources ?? []));
	await post('Profiler.enable');
	await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
	const loaded = validatorLoader
		? await validatorLoader()
		: await import(pathToFileURL(validator).href);
	const validate = typeof loaded === 'function' ? loaded : loaded.validate ?? loaded.default;
	if (typeof validate !== 'function') throw new Error('validator must export validate(input, context) or a default function');
	const setupCoverage = onlyDeclaredScripts(await post('Profiler.takePreciseCoverage'), declaredSourcePaths);
	coverageSnapshots.push(setupCoverage);
	for (let index = 0; index < requests.length;) {
		const first = requests[index];
		const firstCaseKey = first.key.case_id;
		const batch = [first];
		if (first.parallel === true) {
			for (let next = index + 1; next < requests.length; next += 1) {
				const candidate = requests[next];
				if (candidate.key.dataset_id !== first.key.dataset_id || candidate.key.case_id !== firstCaseKey || candidate.parallel !== true) break;
				batch.push(candidate);
			}
		}
		const batchResults = await Promise.all(batch.map(async (item) => {
			try {
				const output = await validate(item.input, {
					case_id: item.key.case_id,
					step_id: item.key.step_id,
					setup: item.setup ?? null,
					parallel: item.parallel === true,
				});
				const serialized = JSON.stringify(output);
				if (serialized === undefined) throw new TypeError('validator output is not JSON serializable');
				return { item, result: { key: item.key, output: JSON.parse(serialized) } };
			} catch (error) {
				return { item, result: { key: item.key, error: `${error?.name ?? 'Error'}: ${error?.message ?? error}` } };
			}
		}));
		const snapshot = onlyDeclaredScripts(await post('Profiler.takePreciseCoverage'), declaredSourcePaths);
		coverageSnapshots.push(snapshot);
		const caseCoverage = await normalizedLineCoverage(
			[setupCoverage, snapshot], packageRoot, request.coverage_sources ?? [],
		);
		for (const file of caseCoverage) {
			const combined = datasetCoverageByFile.get(file.path) ?? {
				path: file.path,
				executable_lines: new Set(),
				covered_lines: new Set(),
			};
			for (const line of file.executable_lines) combined.executable_lines.add(line);
			for (const line of file.covered_lines) combined.covered_lines.add(line);
			datasetCoverageByFile.set(file.path, combined);
		}
		for (const { item, result } of batchResults) {
			results.push(result);
			if (request.coverage_fingerprints !== false) {
				coverageScopes.set(JSON.stringify(item.key), {
					key: item.key,
					files: caseCoverage.map(({ path: source, covered_lines: lines }) => ({ path: source, lines })),
				});
			}
		}
		index += batch.length;
	}
	coverageLines = [...datasetCoverageByFile.values()]
		.map((file) => ({
			path: file.path,
			executable_lines: [...file.executable_lines].sort((left, right) => left - right),
			covered_lines: [...file.covered_lines].sort((left, right) => left - right),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	if (supplementalTests.length) {
		const datasetPercent = coveragePercent(coverageLines);
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
					additionalCoverageSnapshots.push(onlyDeclaredScripts(
						await post('Profiler.takePreciseCoverage'), declaredSourcePaths,
					));
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
console.log = originalConsoleLog;
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
const coverageMetrics = await normalizedCoverageMetrics(
	coverageSnapshots, packageRoot, request.coverage_sources ?? [], coverageLines,
);
const scopes = request.coverage_fingerprints === false
	? []
	: requests.map((item) => coverageScopes.get(JSON.stringify(item.key)) ?? ({ key: item.key, files: [] }));
const additionalCoverageLines = supplementalTests.length && additionalTestCoverage?.executed
	? await normalizedLineCoverage(additionalCoverageSnapshots, packageRoot, request.coverage_sources ?? [])
	: [];
return {
	results,
	coverage,
	coverage_scopes: scopes,
	coverage_lines: coverageLines,
	coverage_metrics: coverageMetrics,
	additional_test_coverage: additionalTestCoverage,
	additional_coverage_lines: additionalCoverageLines,
};
}

async function normalizedCoverageMetrics(snapshots, sourceRoot, declaredSources, coverageLines) {
	const profiles = new Map();
	for (const snapshot of snapshots) {
		const snapshotProfiles = new Map();
		for (const script of snapshot.result ?? []) {
			if (!script.url?.startsWith('file:')) continue;
			const filename = path.resolve(fileURLToPath(script.url));
			if (filename !== sourceRoot && !filename.startsWith(`${sourceRoot}${path.sep}`)) continue;
			const relative = path.relative(sourceRoot, filename).split(path.sep).join('/');
			if (!declaredSources.some((entry) => relative === entry || relative.startsWith(`${entry.replace(/\/$/, '')}/`))) continue;
			const functions = snapshotProfiles.get(filename) ?? [];
			functions.push(...(script.functions ?? []));
			snapshotProfiles.set(filename, functions);
		}
		for (const [filename, functions] of snapshotProfiles) {
			const sourceProfiles = profiles.get(filename) ?? [];
			sourceProfiles.push(functions);
			profiles.set(filename, sourceProfiles);
		}
	}
	const metrics = new Map();
	const add = (name, file) => {
		const metric = metrics.get(name) ?? { covered: 0, total: 0, files: [] };
		metric.covered += file.covered;
		metric.total += file.total;
		metric.files.push(file);
		metrics.set(name, metric);
	};
	for (const file of coverageLines) {
		add('line', {
			path: file.path,
			covered: file.covered_lines.length,
			total: file.executable_lines.length,
		});
	}
	for (const filename of await declaredSourceFiles(sourceRoot, declaredSources)) {
		const statements = new Map();
		const functions = new Map();
		const branches = new Map();
		for (const profile of profiles.get(filename) ?? []) {
			const converter = v8ToIstanbul(filename);
			await converter.load();
			converter.applyCoverage(profile);
			const converted = converter.toIstanbul();
			const record = converted[filename] ?? Object.values(converted)[0];
			if (!record) continue;
			for (const [id, location] of Object.entries(record.statementMap ?? {})) {
				const key = JSON.stringify(location);
				statements.set(key, { location, covered: (statements.get(key)?.covered ?? false) || Number(record.s?.[id] ?? 0) > 0 });
			}
			for (const [id, location] of Object.entries(record.fnMap ?? {})) {
				const key = JSON.stringify(location);
				functions.set(key, (functions.get(key) ?? false) || Number(record.f?.[id] ?? 0) > 0);
			}
			for (const [id, branch] of Object.entries(record.branchMap ?? {})) {
				const counts = record.b?.[id] ?? [];
				for (let index = 0; index < branch.locations.length; index += 1) {
					const location = branch.locations[index];
					const key = JSON.stringify(location);
					branches.set(key, { location, covered: (branches.get(key)?.covered ?? false) || Number(counts[index] ?? 0) > 0 });
				}
			}
		}
		for (const [key, branch] of branches) {
			if (branch.covered) continue;
			const startLine = Number(branch.location.start?.line);
			const endLine = Number(branch.location.end?.line);
			const coveredInRange = [...statements.values()].some(({ location, covered }) => {
				const line = Number(location.start?.line);
				return covered && Number.isInteger(line) && line >= startLine && line <= endLine;
			});
			branches.set(key, { ...branch, covered: coveredInRange });
		}
		const relative = path.relative(sourceRoot, filename).split(path.sep).join('/');
		for (const [name, values] of [
			['statement', statements],
			['function', functions],
			['branch', branches],
		]) {
			if (!values.size) continue;
			add(name, {
				path: relative,
				covered: [...values.values()].filter((value) => typeof value === 'boolean' ? value : value.covered).length,
				total: values.size,
			});
		}
	}
	return Object.fromEntries([...metrics.entries()].map(([name, metric]) => [
		name,
		{ ...metric, files: metric.files.sort((left, right) => left.path.localeCompare(right.path)) },
	]));
}

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

function onlyDeclaredScripts(snapshot, declaredSources) {
	return {
		...snapshot,
		result: (snapshot.result ?? []).filter((script) => {
			if (!script.url?.startsWith('file:')) return false;
			return declaredSources.has(path.resolve(fileURLToPath(script.url)));
		}),
	};
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
