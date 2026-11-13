#!/usr/bin/env node
import Ajv2020 from 'ajv/dist/2020.js';
import { execFile, spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { accessSync, realpathSync } from 'node:fs';
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createInterface } from 'node:readline';
import YAML from 'yaml';
import { readManifest, stageContractTree } from './manifest.js';
import { contractLauncherCommand } from './contract-launcher.js';
import { runBundle } from './runner.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const bundleSchemaPath = path.resolve(here, '../../../schemas/bundle.v1.schema.json');
const predicateSchemaPath = path.resolve(here, '../../../runner/bootstrap-contracts/predicates/predicate-language.schema.json');

export async function main(argv = process.argv.slice(2)) {
	try {
		const options = parseArgs(argv);
		if (options.manifest) {
			if ((options.coverageSources.length || options.coverageThreshold !== undefined) && options.coverageSources.length === 0) {
				options.coverageSources = await collectManifestSources(options.manifest);
			}
			const execute = (coverageDirectory) => runManifest(options.manifest, options.isolation, coverageDirectory);
			const result = options.coverageSources.length || options.coverageThreshold !== undefined
				? await runWithCoverage(options, execute)
				: { report: await execute(), coveragePassed: true };
			printReport(result.report, options.quiet);
			return result.coveragePassed && result.report.passed === result.report.total ? 0 : 1;
		}
		const reader = options.reader ? await loadCallable(options.reader) : undefined;
		const executor = options.executor ? await loadCallable(options.executor) : undefined;
		const bundle = await readBundle(options.dataset, { reader });
		const execute = async (coverageDirectory, tempRoot) => {
			if (!bundle.targets.some(({ kind }) => kind === 'custom')) {
				return runBundle(bundle, { coverageDirectory, executor });
			}
			if (!options.executor) throw new Error('custom targets with coverage require --executor');
			const bundlePath = path.join(tempRoot, 'bundle.json');
			await writeFile(bundlePath, JSON.stringify(bundle));
			const workerPath = path.join(here, 'coverage-worker.js');
			const result = await execFileAsync(process.execPath, [workerPath, bundlePath, options.executor], {
				cwd: process.cwd(),
				env: { ...withoutNodeCoverage(), NODE_V8_COVERAGE: coverageDirectory },
				maxBuffer: 64 * 1024 * 1024,
			});
			try { return JSON.parse(result.stdout); }
			catch { throw new Error(`custom coverage worker returned invalid JSON: ${result.stdout.slice(0, 500)}`); }
		};
		const result = options.coverageSources.length || options.coverageThreshold !== undefined
			? await runWithCoverage(options, execute)
			: { report: await runBundle(bundle, { executor }), coveragePassed: true };
		const { report, coveragePassed } = result;
		printReport(report, options.quiet);
		return coveragePassed && report.passed === report.total ? 0 : 1;
	} catch (error) {
		console.error(`open-contracts-js: ${error.message}`);
		return 2;
	}
}

function parseArgs(argv) {
	const isManifest = argv[0] === 'run-manifest';
	if ((!isManifest && argv[0] !== 'run') || !argv[1]) throw new Error('usage: open-contracts-js run <dataset.json|dataset.yaml> [options] | run-manifest <contract.yaml> [options]');
	const options = {
		...(isManifest ? { manifest: path.resolve(argv[1]), isolation: undefined } : { dataset: path.resolve(argv[1]) }),
		quiet: false,
		coverageSources: [],
		coverageThreshold: undefined,
		coverageCommand: 'c8',
		coverageAllowExternal: false,
		reader: undefined,
		executor: undefined,
	};
	for (let index = 2; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === '--quiet') options.quiet = true;
		else if (arg === '--coverage-source') options.coverageSources.push(argv[++index]);
		else if (arg === '--coverage-threshold') options.coverageThreshold = percentage(argv[++index]);
		else if (arg === '--coverage-command') options.coverageCommand = argv[++index];
		else if (arg === '--coverage-allow-external') options.coverageAllowExternal = true;
		else if (arg === '--isolation' && isManifest) options.isolation = argv[++index];
		else if (arg === '--reader') options.reader = argv[++index];
		else if (arg === '--executor') options.executor = argv[++index];
		else throw new Error(`unknown option: ${arg}`);
	}
	if (isManifest && !options.isolation) throw new Error('run-manifest requires --isolation <none|bwrap|landlock>');
	return options;
}

function percentage(value) {
	const number = Number(value);
	if (!Number.isFinite(number) || number < 0 || number > 100) throw new Error('coverage threshold must be between 0 and 100');
	return number;
}

export async function readBundle(datasetPath, { reader } = {}) {
	datasetPath = datasetPath instanceof URL ? fileURLToPath(datasetPath) : datasetPath;
	const text = await readFile(datasetPath, 'utf8');
	let value = path.extname(datasetPath).toLowerCase() === '.json' ? JSON.parse(text) : YAML.parse(text);
	if (reader) value = await reader(value);
	const [bundleSchema, predicateSchema] = await Promise.all([
		readFile(bundleSchemaPath, 'utf8').then(JSON.parse),
		readFile(predicateSchemaPath, 'utf8').then(JSON.parse),
	]);
	const ajv = new Ajv2020({ allErrors: true, strict: false });
	ajv.addSchema(predicateSchema);
	const validate = ajv.compile(bundleSchema);
	if (!validate(value)) {
		const detail = validate.errors?.map(({ instancePath, message }) => `${instancePath || '/'} ${message}`).join('; ');
		throw new Error(`invalid Open Contract v1 bundle: ${detail}`);
	}
	const targets = value.targets ?? [{
		id: 'default', kind: value.kind ?? 'custom', config: value.config ?? {}, cases: value.cases,
	}];
	const datasetDirectory = path.dirname(path.resolve(datasetPath));
	const resolvedTargets = targets.map((target) => {
		const cwd = target.config?.cwd;
		if (typeof cwd !== 'string' || path.isAbsolute(cwd)) return target;
		return { ...target, config: { ...target.config, cwd: path.resolve(datasetDirectory, cwd) } };
	});
	return { version: 1, targets: resolvedTargets };
}

async function loadCallable(spec) {
	const separator = spec.lastIndexOf(':');
	if (separator <= 0 || separator === spec.length - 1) throw new Error(`plugin must use MODULE:CALLABLE syntax: ${spec}`);
	const moduleName = spec.slice(0, separator);
	const exportName = spec.slice(separator + 1);
	const moduleUrl = moduleName.startsWith('.') || moduleName.startsWith('/')
		? pathToFileURL(path.resolve(moduleName))
		: moduleName;
	const loaded = await import(moduleUrl);
	const callable = loaded[exportName];
	if (typeof callable !== 'function') throw new Error(`plugin is not callable: ${spec}`);
	return callable;
}

async function runWithCoverage(options, execute) {
	if (options.coverageSources.length === 0) throw new Error('coverage requires at least one --coverage-source');
	if (options.coverageThreshold === undefined) options.coverageThreshold = 100;
	const sourcePaths = options.coverageSources.map((source) => path.resolve(source));
	const includes = [];
	const sourceRoots = [];
	for (const source of sourcePaths) {
		const sourceStat = await stat(source).catch(() => null);
		if (!sourceStat) throw new Error(`coverage source does not exist: ${source}`);
		sourceRoots.push(sourceStat.isDirectory() ? source : path.dirname(source));
		const relative = path.relative(process.cwd(), source).split(path.sep).join('/');
		const external = relative === '..' || relative.startsWith('../');
		const include = options.coverageAllowExternal ? source : relative;
		includes.push(sourceStat.isDirectory() ? `${include}/**` : include);
	}
	const tempRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-'));
	const rawDirectory = path.join(tempRoot, 'v8');
	const aggregateRawDirectory = path.join(tempRoot, 'aggregate-v8');
	const reportsDirectory = path.join(tempRoot, 'reports');
	await mkdir(rawDirectory);
	let report;
	let coveragePassed = false;
	try {
		report = await execute(rawDirectory, tempRoot);
		const coverageArtifact = process.env.OPEN_CONTRACT_COVERAGE_ARTIFACT;
		const delegatedCoveragePath = coverageArtifact ? `${coverageArtifact}.delegated.json` : null;
		let summary;
		let coverageReportPath;
		if (delegatedCoveragePath) {
			try {
				const delegatedCoverage = JSON.parse(await readFile(delegatedCoveragePath, 'utf8'));
				summary = summarizeIstanbulCoverage(delegatedCoverage);
				coverageReportPath = delegatedCoveragePath;
				await copyFile(delegatedCoveragePath, coverageArtifact);
			} catch (error) {
				if (error.code !== 'ENOENT') throw error;
			}
		}
		if (!summary) {
			await flattenCoverageFiles(rawDirectory, aggregateRawDirectory);
			const coverageCommand = await resolveCommand(options.coverageCommand);
			const reportArgs = ['report', '--temp-directory', aggregateRawDirectory, '--reports-dir', reportsDirectory,
				'--reporter', 'json-summary', '--reporter', 'json', '--all'];
			if (options.coverageAllowExternal) reportArgs.push('--allowExternal');
			for (const include of includes) reportArgs.push('--include', include);
			await execFileAsync(coverageCommand, reportArgs, {
				cwd: process.cwd(), env: withoutNodeCoverage(), maxBuffer: 8 * 1024 * 1024,
			});
			summary = JSON.parse(await readFile(path.join(reportsDirectory, 'coverage-summary.json'), 'utf8'));
			coverageReportPath = path.join(reportsDirectory, 'coverage-final.json');
			if (coverageArtifact) {
				await mkdir(path.dirname(path.resolve(coverageArtifact)), { recursive: true });
				await copyFile(coverageReportPath, coverageArtifact);
			}
		}
		const executableLines = await readExecutableLines(coverageReportPath);
		const reported = Object.keys(summary).filter((source) => source !== 'total').map((source) => path.resolve(source));
		for (const source of sourcePaths) {
			if (!reported.some((file) => file === source || file.startsWith(`${source}${path.sep}`))) {
				throw new Error(`c8 report did not include requested source: ${source} (reported: ${reported.join(', ') || 'none'})`);
			}
		}
		const totals = summary.total;
		const sourceRoot = commonPath(sourceRoots);
		const scopeIds = collectCoverageScopeIds(report);
		const scopeCoverage = new Map();
		for (const scopeId of scopeIds) {
			scopeCoverage.set(scopeId, await readV8CoverageLineSet(path.join(rawDirectory, 'scopes', scopeId), executableLines, sourceRoot));
		}
		const setupCoverage = await readV8CoverageLineSet(path.join(rawDirectory, 'setup'), executableLines, sourceRoot);
		const delegatedCaseCoverage = new Map();
		if (coverageArtifact) {
			try {
				const scopes = JSON.parse(await readFile(`${coverageArtifact}.scopes.json`, 'utf8'));
				for (const [key, coverage] of Object.entries(scopes)) {
					delegatedCaseCoverage.set(key, istanbulLineSet(coverage, sourceRoot));
				}
			} catch (error) {
				if (error.code !== 'ENOENT') throw error;
			}
		}
		const aggregateCoverage = delegatedCaseCoverage.size
			? unionCoverage([...delegatedCaseCoverage.values()])
			: unionCoverage([setupCoverage, ...scopeCoverage.values()]);
		attachCoverageReport(report, scopeCoverage, aggregateCoverage, delegatedCaseCoverage);
		const metrics = Object.fromEntries(['lines', 'functions', 'branches'].map((name) => {
			const metric = totals[name];
			const pct = metric.pct === 'Unknown' && metric.total === 0
				? (name === 'lines' ? 0 : 100)
				: Number(metric.pct);
			if (!Number.isFinite(pct)) throw new Error(`c8 returned an invalid ${name} percentage`);
			return [name, pct];
		}));
		for (const [name, pct] of Object.entries(metrics)) console.error(`JavaScript ${name}: ${pct.toFixed(2)}%`);
		const below = Object.entries(metrics).filter(([, pct]) => pct < options.coverageThreshold).map(([name]) => name);
		if (below.length) {
			console.error(`Coverage threshold ${options.coverageThreshold}% not met for: ${below.join(', ')}`);
		} else {
			console.error(`JavaScript coverage threshold passed: all metrics >= ${options.coverageThreshold}%`);
			coveragePassed = true;
		}
	} finally {
		await rm(tempRoot, { recursive: true, force: true });
	}
	return { report, coveragePassed };
}

async function runManifest(manifestPath, isolation, coverageDirectory) {
	const manifestReal = await import('node:fs/promises').then(({ realpath }) => realpath(manifestPath));
	const { manifest, root: sourceRoot } = await readManifest(manifestReal);
	const staged = await stageContractTree(manifest, sourceRoot, manifestReal);
	try {
		const command = [process.execPath, path.join(here, 'execution-adapter.js'), staged.root];
		const guestRoot = isolation === 'bwrap' ? `/contract/${randomBytes(12).toString('hex')}` : undefined;
		const launcher = contractLauncherCommand(isolation, staged.root, manifest, command, {
			read: staged.read, write: staged.write, runtimeProfile: 'javascript', guestRoot,
		});
		const env = { ...process.env };
		if (coverageDirectory) env.NODE_V8_COVERAGE = coverageDirectory;
		const processHandle = spawn(launcher[0], launcher.slice(1), { env, stdio: ['pipe', 'pipe', 'pipe'] });
		const closed = new Promise((resolve, reject) => {
			processHandle.once('error', reject);
			processHandle.once('close', (code, signal) => resolve({ code, signal }));
		});
		const lines = createInterface({ input: processHandle.stdout })[Symbol.asyncIterator]();
		let stderr = '';
		processHandle.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
		const serial = { pending: Promise.resolve() };
		const executor = (validator, input) => {
			const operation = serial.pending.then(async () => {
				processHandle.stdin.write(`${JSON.stringify({ validator, input })}\n`);
				const next = await lines.next();
				if (next.done) throw new Error('isolated executor stopped without returning output');
				const response = JSON.parse(next.value);
				if (Object.hasOwn(response, 'error')) throw new Error(response.error);
				return response.output;
			});
			serial.pending = operation.catch(() => {});
			return operation;
		};
		const contracts = [];
		const collect = async (current, prefix, parents) => {
			const real = await import('node:fs/promises').then(({ realpath }) => realpath(current));
			if (parents.has(real)) throw new Error(`nested contract cycle detected at ${real}`);
			const { manifest, root } = await readManifest(real);
			contracts.push({ manifest, root, prefix });
			for (const child of manifest.files.nested_contracts ?? []) {
				const childPrefix = `${manifest.name}/${path.basename(path.dirname(child)) || path.basename(child, path.extname(child))}`;
				await collect(path.join(root, child), [prefix, childPrefix].filter(Boolean).join('/'), new Set([...parents, real]));
			}
		};
		await collect(manifestReal, '', new Set());
		const allCases = [];
		try {
			for (const contract of contracts) {
				const validatorSource = path.resolve(contract.root, contract.manifest.files.validator);
				const stagedValidator = [...staged.sourceMap].find(([, original]) => path.resolve(original) === validatorSource)?.[0];
				if (!stagedValidator) throw new Error(`validator is not staged: ${validatorSource}`);
				const bundle = await readBundle(path.join(contract.root, contract.manifest.files.dataset));
				const report = await runBundle({ ...bundle, targets: bundle.targets.map((target) => ({ ...target, kind: 'custom' })) }, {
					executor: (input) => executor(path.relative(staged.root, stagedValidator).split(path.sep).join('/'), input),
				});
				for (const [targetId, target] of Object.entries(report.targets)) {
					for (const item of target.cases) allCases.push({ ...item, target_id: contract.prefix ? `${contract.prefix}/${item.target_id || targetId}` : (item.target_id || targetId) });
				}
			}
		} finally {
			processHandle.stdin.end();
		}
		const { code, signal } = await closed;
		if (code !== 0) throw new Error(`isolated contract executor ${signal ? `signal ${signal}` : `exit ${code}`}: ${stderr.trim()}`);
		if (coverageDirectory) await rewriteV8Coverage(coverageDirectory, staged.root, staged.sourceMap, guestRoot);
		return aggregateReport(allCases);
	} finally {
		const { rm } = await import('node:fs/promises');
		await rm(staged.root, { recursive: true, force: true });
	}
}

async function rewriteV8Coverage(directory, stagedRoot, sourceMap, guestRoot) {
	const byStaged = new Map([...sourceMap].map(([staged, original]) => [path.resolve(staged), original]));
	for (const name of await readdir(directory)) {
		if (!name.endsWith('.json')) continue;
		const file = path.join(directory, name);
		const coverage = JSON.parse(await readFile(file, 'utf8'));
		let changed = false;
		for (const script of coverage.result ?? []) {
			if (!script.url?.startsWith('file:')) continue;
			let source;
			try { source = fileURLToPath(script.url); } catch { continue; }
			let staged = path.resolve(source);
			if (guestRoot && (staged === guestRoot || staged.startsWith(`${guestRoot}${path.sep}`))) {
				staged = path.join(stagedRoot, path.relative(guestRoot, staged));
			}
			const original = byStaged.get(staged);
			if (original) { script.url = pathToFileURL(original).href; changed = true; }
		}
		if (changed) await writeFile(file, JSON.stringify(coverage));
	}
}

async function collectManifestSources(manifestPath, visited = new Set()) {
	const resolved = await import('node:fs/promises').then(({ realpath }) => realpath(manifestPath));
	if (visited.has(resolved)) return [];
	visited.add(resolved);
	const { manifest, root } = await readManifest(resolved);
	const sources = [path.join(root, manifest.files.validator), ...(manifest.files.support ?? []).map((name) => path.join(root, name))];
	for (const child of manifest.files.nested_contracts ?? []) sources.push(...await collectManifestSources(path.join(root, child), visited));
	return [...new Set(sources)];
}

function aggregateReport(cases) {
	const targets = {};
	for (const item of cases) (targets[item.target_id] ??= []).push(item);
	return {
		passed: cases.filter(({ passed }) => passed).length,
		total: cases.length,
		targets: Object.fromEntries(Object.entries(targets).map(([id, targetCases]) => [id, {
			passed: targetCases.filter(({ passed }) => passed).length, total: targetCases.length, cases: targetCases,
		}])),
	};
}

function summarizeIstanbulCoverage(coverage) {
	const lineHits = new Map();
	const functions = [];
	const branches = [];
	const bySource = {};
	for (const [source, record] of Object.entries(coverage)) {
		const sourceLines = new Map();
		for (const [key, location] of Object.entries(record.statementMap ?? {})) {
			const line = Number(location.start?.line ?? key);
			if (!Number.isInteger(line)) continue;
			const hit = Number(record.s?.[key] ?? 0) > 0;
			const id = `${record.path ?? source}:${line}`;
			lineHits.set(id, (lineHits.get(id) ?? false) || hit);
			sourceLines.set(line, (sourceLines.get(line) ?? false) || hit);
		}
		const sourceFunctions = Object.entries(record.fnMap ?? {}).map(([key]) => Number(record.f?.[key] ?? 0) > 0);
		const sourceBranches = Object.entries(record.b ?? {}).flatMap(([, hits]) => hits.map((hit) => Number(hit) > 0));
		functions.push(...sourceFunctions);
		branches.push(...sourceBranches);
		bySource[source] = {
			lines: metric([...sourceLines.values()]),
			functions: metric(sourceFunctions),
			branches: metric(sourceBranches),
		};
	}
	return {
		...bySource,
		total: {
			lines: metric([...lineHits.values()]),
			functions: metric(functions),
			branches: metric(branches),
		},
	};
}

function metric(values) {
	const covered = values.filter(Boolean).length;
	const total = values.length;
	return { covered, total, pct: total === 0 ? 100 : covered * 100 / total };
}

async function flattenCoverageFiles(sourceDirectory, destinationDirectory) {
	await mkdir(destinationDirectory, { recursive: true });
	const visit = async (directory) => {
		for (const entry of await readdir(directory, { withFileTypes: true })) {
			const source = path.join(directory, entry.name);
			if (entry.isDirectory()) await visit(source);
			else if (entry.isFile() && entry.name.endsWith('.json')) {
				await copyFile(source, path.join(destinationDirectory, entry.name));
			}
		}
	};
	await visit(sourceDirectory);
}

async function readExecutableLines(filename) {
	const coverage = JSON.parse(await readFile(filename, 'utf8'));
	return new Map(Object.entries(coverage).map(([source, record]) => [
		path.resolve(source), new Set(Object.values(record.statementMap ?? {})
			.map(({ start }) => Number(start?.line)).filter(Number.isInteger)),
	]));
}

function istanbulLineSet(coverage, sourceRoot) {
	const files = [];
	for (const [source, record] of Object.entries(coverage ?? {})) {
		const lines = new Set();
		for (const [id, location] of Object.entries(record.statementMap ?? {})) {
			if (Number(record.s?.[id] ?? 0) > 0) {
				const line = Number(location.start?.line);
				if (Number.isInteger(line)) lines.add(line);
			}
		}
		if (lines.size) {
			const filename = path.resolve(record.path ?? source);
			files.push({ path: path.relative(sourceRoot, filename).split(path.sep).join('/'), lines: [...lines] });
		}
	}
	return canonicalCoverage(files);
}

async function readV8CoverageLineSet(directory, executableLines, sourceRoot) {
	const coveredByFile = new Map();
	for (const entry of await readdir(directory, { withFileTypes: true }).catch(() => [])) {
		if (!entry.isFile() || !entry.name.endsWith('.json')) continue;
		const profile = JSON.parse(await readFile(path.join(directory, entry.name), 'utf8'));
		for (const script of profile.result ?? []) {
			if (!script.url?.startsWith('file://')) continue;
			const sourcePath = fileURLToPath(script.url);
			const executable = executableLines.get(path.resolve(sourcePath));
			if (!executable?.size) continue;
			const source = await readFile(sourcePath, 'utf8');
			const starts = lineStarts(source);
			const lines = coveredByFile.get(sourcePath) ?? new Set();
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
			for (const line of [...lines]) if (!executable.has(line)) lines.delete(line);
			if (lines.size) coveredByFile.set(sourcePath, lines);
		}
	}
	return canonicalCoverage([...coveredByFile].map(([source, lines]) => ({
		path: path.relative(sourceRoot, source).split(path.sep).join('/'),
		lines: [...lines],
	})));
}

function commonPath(paths) {
	if (paths.length === 0) return process.cwd();
	const [first, ...rest] = paths.map((item) => path.resolve(item));
	const firstParts = first.split(path.sep);
	let sharedLength = firstParts.length;
	for (const item of rest) {
		const parts = item.split(path.sep);
		sharedLength = Math.min(sharedLength, parts.length);
		let index = 0;
		while (index < sharedLength && firstParts[index] === parts[index]) index += 1;
		sharedLength = index;
	}
	return firstParts.slice(0, sharedLength).join(path.sep) || path.parse(first).root;
}

function lineStarts(source) {
	const starts = [0];
	for (let index = 0; index < source.length; index += 1) if (source[index] === '\n') starts.push(index + 1);
	return starts;
}

function addOverlappingLines(lines, starts, sourceLength, rangeStart, rangeEnd) {
	let lineIndex = 0;
	while (lineIndex + 1 < starts.length && starts[lineIndex + 1] <= rangeStart) lineIndex += 1;
	while (lineIndex < starts.length && starts[lineIndex] < rangeEnd) {
		const lineStart = starts[lineIndex];
		const lineEnd = starts[lineIndex + 1] ?? sourceLength;
		if (rangeStart < lineEnd && rangeEnd > lineStart) lines.add(lineIndex + 1);
		lineIndex += 1;
	}
}

function canonicalCoverage(files) {
	const sorted = files.map(({ path: source, lines }) => ({
		path: source,
		lines: [...new Set(lines)].sort((a, b) => a - b),
	})).filter(({ lines }) => lines.length).sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
	const serialized = JSON.stringify(sorted);
	return { files: sorted, fingerprint: createHash('sha256').update(serialized).digest('hex').slice(0, 32) };
}

function collectCoverageScopeIds(report) {
	const ids = new Set();
	for (const target of Object.values(report.targets)) {
		for (const testCase of target.cases) {
			for (const id of testCase.coverage_scopes?.case ?? []) ids.add(id);
			for (const scope of Object.values(testCase.coverage_scopes?.steps ?? {})) ids.add(scope.id);
		}
	}
	return [...ids].sort();
}

function unionCoverage(sets) {
	const linesByPath = new Map();
	for (const set of sets) {
		for (const file of set?.files ?? []) {
			const lines = linesByPath.get(file.path) ?? new Set();
			for (const line of file.lines) lines.add(line);
			linesByPath.set(file.path, lines);
		}
	}
	return canonicalCoverage([...linesByPath].map(([source, lines]) => ({ path: source, lines: [...lines] })));
}

function attachCoverageReport(report, scopes, aggregateLines, delegatedCaseCoverage = new Map()) {
	for (const target of Object.values(report.targets)) {
		for (const testCase of target.cases) {
			const scopeRecord = testCase.coverage_scopes ?? { case: [], steps: {} };
			const caseCoverage = delegatedCaseCoverage.get(`${testCase.target_id}/${testCase.case_id}`)
				?? unionCoverage(scopeRecord.case.map((id) => scopes.get(id)));
			const baseline = testCase.coverage_baselines ?? { case: null, steps: {} };
			const failures = [];
			if (baseline.case) {
				caseCoverage.expected_fingerprint = baseline.case;
				caseCoverage.baseline_matches = baseline.case === caseCoverage.fingerprint;
				if (!caseCoverage.baseline_matches) failures.push(`case expected ${baseline.case}, observed ${caseCoverage.fingerprint}`);
			}
			const steps = Object.fromEntries(Object.entries(scopeRecord.steps).map(([id, scope]) => {
				const coverage = { ...scopes.get(scope.id), attribution: scope.attribution };
				const expectedFingerprint = baseline.steps[id] ?? scope.expectedFingerprint;
				if (expectedFingerprint) {
					coverage.expected_fingerprint = expectedFingerprint;
					coverage.baseline_matches = expectedFingerprint === coverage.fingerprint;
					if (!coverage.baseline_matches) failures.push(`step ${id} expected ${expectedFingerprint}, observed ${coverage.fingerprint}`);
				}
				return [id, coverage];
			}));
			testCase.coverage = { ...caseCoverage, ...(Object.keys(steps).length ? { steps } : {}) };
			if (failures.length) {
				testCase.passed = false;
				testCase.matched_by = null;
				testCase.error = `coverage fingerprint mismatch: ${failures.join('; ')}`;
				for (const check of testCase.actual?.checks ?? []) {
					if (steps[check.id]?.baseline_matches === false) check.passed = false;
				}
			}
			delete testCase.coverage_scopes;
			delete testCase.coverage_baselines;
		}
		target.passed = target.cases.filter(({ passed }) => passed).length;
	}
	report.passed = Object.values(report.targets).reduce((count, target) => count + target.passed, 0);
	report.coverage = { ...aggregateLines };
}

async function resolveCommand(command) {
	if (command.includes(path.sep)) return path.resolve(command);
	for (const directory of (process.env.PATH ?? '').split(path.delimiter)) {
		const candidate = path.join(directory, command);
		try {
			accessSync(candidate);
			return candidate;
		} catch { /* continue searching PATH */ }
	}
	throw new Error(`coverage command not found: ${command}`);
}

function withoutNodeCoverage() {
	const env = { ...process.env };
	delete env.NODE_V8_COVERAGE;
	return env;
}

function execFileAsync(command, args, options) {
	return new Promise((resolve, reject) => execFile(command, args, options, (error, stdout, stderr) => {
		if (error) reject(new Error(`coverage command failed: ${stderr.trim() || error.message}`));
		else resolve({ stdout, stderr });
	}));
}

function printReport(report, quiet) {
	if (!quiet) {
		console.log(JSON.stringify(report, null, 2));
		return;
	}
	console.log(`Cases passed: ${report.passed}/${report.total}`);
	for (const target of Object.values(report.targets)) {
		for (const result of target.cases.filter(({ passed }) => !passed)) {
			console.error(`FAIL ${result.target_id}/${result.case_id}`);
			if (result.error) console.error(`  error: ${result.error}`);
			else console.error(`  actual: ${JSON.stringify(result.actual)}`);
		}
	}
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
	process.exitCode = await main();
}
