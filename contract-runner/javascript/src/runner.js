import { spawn } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { evaluatePredicate } from './predicate.js';

export async function runBundle(bundle, { coverageDirectory, executor } = {}) {
	const results = [];
	for (const target of bundle.targets) {
		for (const testCase of target.cases) {
			try {
				const execution = testCase.steps
					? await executeSequence(target, testCase, coverageDirectory, executor)
					: { actual: await executeTarget(target, testCase, coverageDirectory, executor), passed: true };
				const actual = execution.actual;
				const matched = testCase.predicate !== undefined
					? evaluatePredicate(testCase.predicate, actual)
					: isDeepStrictEqual(actual, testCase.expected);
				const passed = execution.passed && matched;
				results.push({
					target_id: target.id,
					case_id: testCase.id,
					actual,
					passed,
					matched_by: passed ? (testCase.predicate !== undefined ? 'predicate' : 'exact') : null,
					error: execution.error ?? null,
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

async function executeSequence(target, testCase, coverageDirectory, executor) {
	validateSequence(testCase.steps);
	const stepResults = Object.create(null);
	const captures = Object.create(null);
	const checks = [];
	let stepIndex = 0;
	const executeStep = async (step) => {
		const currentIndex = stepIndex++;
		const input = resolveReferences(step.input, testCase.input, captures);
		const context = { case_id: testCase.id, step_id: step.id, step_index: currentIndex, captures: { ...captures } };
		const actual = await executeTarget(target, { id: `${testCase.id}/${step.id}`, input }, coverageDirectory, executor, context);
		const matched = step.predicate !== undefined
			? evaluatePredicate(step.predicate, actual)
			: step.expected !== undefined ? isDeepStrictEqual(actual, step.expected) : true;
		const stepCaptures = Object.create(null);
		for (const [name, pointer] of Object.entries(step.capture ?? {})) {
			const value = valueAtPointer(actual, pointer);
			if (value === MISSING) throw new Error(`capture path not found: ${name} (${pointer})`);
			stepCaptures[name] = value;
		}
		return {
			id: step.id,
			actual,
			passed: matched,
			matched_by: step.predicate !== undefined ? 'predicate' : step.expected !== undefined ? 'exact' : null,
			captures: stepCaptures,
		};
	};
	for (const item of testCase.steps) {
		const completed = item.parallel
			? await Promise.all(item.parallel.map(executeStep))
			: [await executeStep(item)];
		for (const result of completed) {
			stepResults[result.id] = result.actual;
			checks.push({ id: result.id, passed: result.passed, matched_by: result.matched_by });
			Object.assign(captures, result.captures);
		}
	}
	return {
		actual: {
			input: testCase.input,
			steps: Object.fromEntries(Object.entries(stepResults)),
			captures: Object.fromEntries(Object.entries(captures)),
			checks,
		},
		passed: checks.every(({ passed }) => passed),
		error: null,
	};
}

function validateSequence(steps) {
	const ids = new Set();
	const captures = new Set();
	for (const item of steps) {
		for (const step of item.parallel ?? [item]) {
			if (ids.has(step.id)) throw new Error(`duplicate sequence step id: ${step.id}`);
			ids.add(step.id);
			for (const name of Object.keys(step.capture ?? {})) {
				if (captures.has(name)) throw new Error(`duplicate sequence capture: ${name}`);
				captures.add(name);
			}
		}
	}
}

const MISSING = Symbol('missing');
function resolveReferences(value, input, captures) {
	if (Array.isArray(value)) return value.map((item) => resolveReferences(item, input, captures));
	if (value !== null && typeof value === 'object') {
		const keys = Object.keys(value);
		if (keys.length === 1 && keys[0] === '$capture') {
			const name = value.$capture;
			if (typeof name !== 'string' || !Object.hasOwn(captures, name)) throw new Error(`unknown sequence capture: ${name}`);
			return captures[name];
		}
		if (keys.length === 1 && keys[0] === '$input') {
			const result = valueAtPointer(input, value.$input);
			if (result === MISSING) throw new Error(`input path not found: ${value.$input}`);
			return result;
		}
		return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, resolveReferences(item, input, captures)]));
	}
	return value;
}

function valueAtPointer(value, pointer) {
	if (typeof pointer !== 'string' || (pointer !== '' && !pointer.startsWith('/'))) throw new TypeError('capture path must be a JSON Pointer');
	if (pointer === '') return value;
	return pointer.slice(1).split('/').map((part) => part.replace(/~1/g, '/').replace(/~0/g, '~')).reduce((current, key) => {
		if (Array.isArray(current) && /^(0|[1-9][0-9]*)$/.test(key) && Number(key) < current.length) return current[Number(key)];
		return current !== null && typeof current === 'object' && Object.hasOwn(current, key) ? current[key] : MISSING;
	}, value);
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

async function executeTarget(target, testCase, coverageDirectory, executor, context) {
	if (target.kind === 'command' || target.kind === 'hooks') {
		return executeCommand(target, testCase, coverageDirectory);
	}
	if (target.kind === 'http') return executeHttp(target, testCase);
	if (target.kind === 'custom' && executor) return normalizeJson(await executor(testCase.input, context), 'actual');
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
