#!/usr/bin/env node
import Ajv2020 from 'ajv/dist/2020.js';
import { execFile } from 'node:child_process';
import { accessSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import YAML from 'yaml';
import { runBundle } from './runner.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const bundleSchemaPath = path.resolve(here, '../../python/schemas/bundle.v1.schema.json');
const predicateSchemaPath = path.resolve(here, '../../../bootstrap-contracts/predicates/predicate-language.schema.json');

export async function main(argv = process.argv.slice(2)) {
	try {
		const options = parseArgs(argv);
		const reader = options.reader ? await loadCallable(options.reader) : undefined;
		const executor = options.executor ? await loadCallable(options.executor) : undefined;
		const bundle = await readBundle(options.dataset, { reader });
		const result = options.coverageSources.length || options.coverageThreshold !== undefined
			? await runWithCoverage(bundle, options, executor)
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
	if (argv[0] !== 'run' || !argv[1]) throw new Error('usage: open-contracts-js run <dataset.json|dataset.yaml> [options]');
	const options = {
		dataset: path.resolve(argv[1]),
		quiet: false,
		coverageSources: [],
		coverageThreshold: undefined,
		coverageCommand: 'c8',
		reader: undefined,
		executor: undefined,
	};
	for (let index = 2; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === '--quiet') options.quiet = true;
		else if (arg === '--coverage-source') options.coverageSources.push(argv[++index]);
		else if (arg === '--coverage-threshold') options.coverageThreshold = percentage(argv[++index]);
		else if (arg === '--coverage-command') options.coverageCommand = argv[++index];
		else if (arg === '--reader') options.reader = argv[++index];
		else if (arg === '--executor') options.executor = argv[++index];
		else throw new Error(`unknown option: ${arg}`);
	}
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
	return { version: 1, targets };
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

async function runWithCoverage(bundle, options, executor) {
	if (options.coverageSources.length === 0) throw new Error('coverage requires at least one --coverage-source');
	if (options.coverageThreshold === undefined) options.coverageThreshold = 100;
	const sourcePaths = options.coverageSources.map((source) => path.resolve(source));
	const includes = [];
	for (const source of sourcePaths) {
		const sourceStat = await stat(source).catch(() => null);
		if (!sourceStat) throw new Error(`coverage source does not exist: ${source}`);
		const relative = path.relative(process.cwd(), source).split(path.sep).join('/');
		includes.push(sourceStat.isDirectory() ? `${relative}/**` : relative);
	}
	const tempRoot = await mkdtemp(path.join(os.tmpdir(), 'open-contract-js-'));
	const rawDirectory = path.join(tempRoot, 'v8');
	const reportsDirectory = path.join(tempRoot, 'reports');
	await mkdir(rawDirectory);
	let report;
	let coveragePassed = false;
	try {
		report = await runBundle(bundle, { coverageDirectory: rawDirectory, executor });
		const reportArgs = ['report', '--temp-directory', rawDirectory, '--reports-dir', reportsDirectory, '--reporter', 'json-summary', '--all'];
		for (const include of includes) reportArgs.push('--include', include);
		await execFileAsync(await resolveCommand(options.coverageCommand), reportArgs, {
			cwd: process.cwd(), env: withoutNodeCoverage(), maxBuffer: 8 * 1024 * 1024,
		});
		const summary = JSON.parse(await readFile(path.join(reportsDirectory, 'coverage-summary.json'), 'utf8'));
		const reported = Object.keys(summary).filter((source) => source !== 'total').map((source) => path.resolve(source));
		for (const source of sourcePaths) {
			if (!reported.some((file) => file === source || file.startsWith(`${source}${path.sep}`))) {
				throw new Error(`c8 report did not include requested source: ${source}`);
			}
		}
		const totals = summary.total;
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

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	process.exitCode = await main();
}
