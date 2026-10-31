import { spawn } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { evaluatePredicate } from './predicate.js';

export async function runBundle(bundle, { coverageDirectory, executor } = {}) {
	const results = [];
	for (const target of bundle.targets) {
		for (const testCase of target.cases) {
			try {
				const actual = await executeTarget(target, testCase, coverageDirectory, executor);
				const passed = testCase.predicate !== undefined
					? evaluatePredicate(testCase.predicate, actual)
					: isDeepStrictEqual(actual, testCase.expected);
				results.push({
					target_id: target.id,
					case_id: testCase.id,
					actual,
					passed,
					matched_by: passed ? (testCase.predicate !== undefined ? 'predicate' : 'exact') : null,
					error: null,
				});
			} catch (error) {
				results.push({
					target_id: target.id,
					case_id: testCase.id,
					actual: null,
					passed: false,
					matched_by: null,
					error: `${error.name}: ${error.message}`,
				});
			}
		}
	}
	return makeReport(results);
}

function makeReport(results) {
	const targets = {};
	for (const result of results) (targets[result.target_id] ??= []).push(result);
	return {
		passed: results.filter(({ passed }) => passed).length,
		total: results.length,
		targets: Object.fromEntries(Object.entries(targets).map(([id, cases]) => [id, {
			passed: cases.filter(({ passed }) => passed).length,
			total: cases.length,
			cases,
		}])),
	};
}

async function executeTarget(target, testCase, coverageDirectory, executor) {
	if (target.kind === 'command' || target.kind === 'hooks') {
		return executeCommand(target, testCase, coverageDirectory);
	}
	if (target.kind === 'http') return executeHttp(target, testCase);
	if (target.kind === 'custom' && executor) return normalizeJson(await executor(testCase.input), 'actual');
	throw new Error(`no executor registered for target kind: ${target.kind}`);
}

function normalizeJson(value, context) {
	if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
	if (typeof value === 'number') {
		if (Number.isFinite(value)) return value;
		throw new TypeError(`${context} contains a non-finite number`);
	}
	if (Array.isArray(value)) return value.map((item, index) => normalizeJson(item, `${context}[${index}]`));
	if (typeof value === 'object' && (Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null)) {
		return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, normalizeJson(item, `${context}.${key}`)]));
	}
	throw new TypeError(`${context} contains non-JSON value ${value?.constructor?.name ?? typeof value}`);
}

function executeCommand(target, testCase, coverageDirectory) {
	const { command, env: configuredEnv = {}, cwd, timeout = 30 } = target.config;
	if (!Array.isArray(command) || command.length === 0 || command.some((arg) => typeof arg !== 'string')) {
		throw new TypeError(`target ${target.id} config.command must be a non-empty string array`);
	}
	if (!configuredEnv || typeof configuredEnv !== 'object' || Array.isArray(configuredEnv)
		|| Object.entries(configuredEnv).some(([key, value]) => typeof key !== 'string' || typeof value !== 'string')) {
		throw new TypeError(`target ${target.id} config.env must map strings to strings`);
	}
	const environment = { ...process.env, ...configuredEnv };
	if (coverageDirectory) environment.NODE_V8_COVERAGE = coverageDirectory;
	return new Promise((resolve, reject) => {
		const child = spawn(command[0], command.slice(1), {
			cwd: cwd ?? process.cwd(), env: environment, stdio: ['pipe', 'pipe', 'pipe'],
		});
		let stdout = '';
		let stderr = '';
		const timer = setTimeout(() => child.kill('SIGTERM'), timeout * 1000);
		child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
		child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
		child.on('error', (error) => { clearTimeout(timer); reject(error); });
		child.on('close', (code, signal) => {
			clearTimeout(timer);
			if (code !== 0) {
				const reason = signal ? `signal ${signal}` : `exit ${code}`;
				reject(new Error(`command ${reason}: ${stderr.trim()}`));
				return;
			}
			try { resolve(JSON.parse(stdout)); }
			catch { resolve(stdout); }
		});
		child.stdin.end(JSON.stringify(testCase.input));
	});
}

async function executeHttp(target, testCase) {
	const { url, method = 'POST', headers = {}, timeout = 30 } = target.config;
	if (typeof url !== 'string' || !url) throw new TypeError(`target ${target.id} config.url must be a non-empty string`);
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), timeout * 1000);
	try {
		const response = await fetch(url, {
			method,
			headers: { 'Content-Type': 'application/json', ...headers },
			body: JSON.stringify(testCase.input),
			signal: controller.signal,
		});
		const text = await response.text();
		let body;
		try { body = JSON.parse(text); } catch { body = text; }
		return { status: response.status, body };
	} finally {
		clearTimeout(timer);
	}
}
