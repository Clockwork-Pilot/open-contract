import { spawn } from 'node:child_process';
import { mkdir, readdir, rename } from 'node:fs/promises';
import path from 'node:path';
import v8 from 'node:v8';
import { evaluatePredicate } from './predicate.js';

export async function runBundle(bundle, { coverageDirectory, executor } = {}) {
	if (!coverageDirectory && bundle.targets.some((target) => target.cases.some((testCase) =>
		testCase.coverage_fingerprint || testCase.steps?.some((item) =>
			(item.parallel ?? [item]).some((step) => step.coverage_fingerprint))))) {
		throw new Error('dataset coverage_fingerprint requires coverage collection with --coverage-source');
	}
	const results = [];
	let scopeIndex = 0;
	const nextScope = () => `scope-${String(scopeIndex++).padStart(5, '0')}`;
	for (const target of bundle.targets) {
		for (const testCase of target.cases) {
			const coverageScopes = { case: [], steps: {} };
			const coverageBaselines = {
				case: testCase.coverage_fingerprint ?? null,
				steps: Object.fromEntries((testCase.steps ?? []).flatMap((item) =>
					(item.parallel ?? [item]).filter((step) => step.coverage_fingerprint)
						.map((step) => [step.id, step.coverage_fingerprint]))),
			};
			try {
				const execution = testCase.steps
					? await executeSequence(target, testCase, coverageDirectory, executor, nextScope, coverageScopes)
					: await executeCase(target, testCase, coverageDirectory, executor, nextScope, coverageScopes);
				const actual = execution.actual;
				const finalMatch = testCase.steps ? { passed: true } : matchExpectedAndPredicate(actual, testCase);
				const passed = execution.passed && finalMatch.passed;
				results.push({
					target_id: target.id,
					case_id: testCase.id,
					actual,
					passed,
					matched_by: passed ? (finalMatch.matchedBy ?? (testCase.steps ? 'sequence' : null)) : null,
					error: execution.error ?? null,
					...(coverageDirectory ? { coverage_scopes: coverageScopes } : {}),
					...(coverageDirectory ? { coverage_baselines: coverageBaselines } : {}),
				});
			} catch (error) {
				results.push({
					target_id: target.id,
					case_id: testCase.id,
					actual: null,
					passed: false,
					matched_by: null,
					error: `${error.name}: ${error.message}`,
					...(coverageDirectory ? { coverage_scopes: coverageScopes } : {}),
					...(coverageDirectory ? { coverage_baselines: coverageBaselines } : {}),
				});
			}
		}
	}
	return makeReport(results);
}

async function executeCase(target, testCase, coverageDirectory, executor, nextScope, coverageScopes) {
	if (!coverageDirectory) {
		return { actual: await executeTarget(target, testCase, undefined, executor), passed: true };
	}
	const scopeId = nextScope();
	coverageScopes.case.push(scopeId);
	const actual = await executeScopedTarget(target, testCase, coverageDirectory, executor, undefined, scopeId);
	return { actual, passed: true };
}

async function executeSequence(target, testCase, coverageDirectory, executor, nextScope, coverageScopes) {
	validateSequence(testCase.steps);
	const stepResults = Object.create(null);
	const captures = Object.create(null);
	const checks = [];
	const stepCount = testCase.steps.reduce((count, item) => count + (item.parallel?.length ?? 1), 0);
	let stepIndex = 0;
	const executeStep = async (step, predicateContext, scopeId, captureCoverage = true) => {
		const currentIndex = stepIndex++;
		if (coverageDirectory) {
			coverageScopes.case.push(scopeId);
			coverageScopes.steps[step.id] = {
				id: scopeId,
				attribution: captureCoverage ? 'step' : 'parallel_group',
				expectedFingerprint: step.coverage_fingerprint ?? null,
			};
		}
		const input = resolveReferences(step.input, testCase.input, captures);
		const context = {
			case_id: testCase.id, step_id: step.id, step_index: currentIndex, step_count: stepCount,
			captures: { ...captures },
		};
		const actual = coverageDirectory
			? await executeScopedTarget(target, { id: `${testCase.id}/${step.id}`, input }, coverageDirectory, executor, context, scopeId, captureCoverage)
			: await executeTarget(target, { id: `${testCase.id}/${step.id}`, input }, undefined, executor, context);
		const matcher = step.expected === undefined ? step : {
			...step,
			expected: resolveReferences(step.expected, testCase.input, captures),
		};
		const stepMatch = matchExpectedAndPredicate(actual, matcher, {
			...actual,
			sequence: { steps: predicateContext.steps, captures: predicateContext.captures },
		});
		const stepCaptures = Object.create(null);
		for (const [name, pointer] of Object.entries(step.capture ?? {})) {
			const value = valueAtPointer(actual, pointer);
			if (value === MISSING) throw new Error(`capture path not found: ${name} (${pointer})`);
			stepCaptures[name] = value;
		}
		return {
			id: step.id,
			actual,
			passed: stepMatch.passed,
			matched_by: stepMatch.matchedBy,
			captures: stepCaptures,
			coverageScope: scopeId,
			coverageFingerprint: step.coverage_fingerprint ?? null,
		};
	};
	for (const item of testCase.steps) {
		const predicateContext = { steps: { ...stepResults }, captures: { ...captures } };
		let completed;
		if (item.parallel) {
			const sharedScope = coverageDirectory && target.kind === 'custom' ? nextScope() : null;
			const settled = await Promise.allSettled(item.parallel.map((step) => executeStep(
				step, predicateContext, sharedScope ?? (coverageDirectory ? nextScope() : null), !sharedScope,
			)));
			if (sharedScope) await flushV8Coverage(path.join(coverageDirectory, 'scopes', sharedScope));
			const rejected = settled.find(({ status }) => status === 'rejected');
			if (rejected) throw rejected.reason;
			completed = settled.map(({ value }) => value);
		} else {
			completed = [await executeStep(item, predicateContext, coverageDirectory ? nextScope() : null)];
		}
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

async function executeScopedTarget(target, testCase, coverageDirectory, executor, context, scopeId, captureCoverage = true) {
	const scopeDirectory = path.join(coverageDirectory, 'scopes', scopeId);
	await mkdir(scopeDirectory, { recursive: true });
	if (target.kind !== 'custom' || !executor) {
		return executeTarget(target, testCase, scopeDirectory, executor, context);
	}
	try {
		return await executeTarget(target, testCase, scopeDirectory, executor, context);
	} finally {
		if (captureCoverage) await flushV8Coverage(scopeDirectory);
	}
}

export async function flushV8Coverage(directory) {
	await mkdir(directory, { recursive: true });
	const rawDirectory = process.env.NODE_V8_COVERAGE;
	if (!rawDirectory) throw new Error('per-step V8 coverage requires NODE_V8_COVERAGE');
	const before = new Set((await readdir(rawDirectory)).filter((name) => name.endsWith('.json')));
	v8.takeCoverage();
	const snapshots = (await readdir(rawDirectory)).filter((name) => name.endsWith('.json') && !before.has(name));
	for (const name of snapshots) await rename(path.join(rawDirectory, name), path.join(directory, name));
}

function matchExpectedAndPredicate(actual, matcher, predicateRecord = actual) {
	const hasExpected = matcher.expected !== undefined;
	const hasPredicate = matcher.predicate !== undefined;
	const expectedPassed = !hasExpected || matchesExpectedSubset(actual, matcher.expected);
	const predicatePassed = !hasPredicate || evaluatePredicate(matcher.predicate, predicateRecord);
	const matchedBy = hasExpected && hasPredicate ? 'expected+predicate'
		: hasExpected ? 'expected-subset'
			: hasPredicate ? 'predicate' : null;
	return { passed: expectedPassed && predicatePassed, matchedBy };
}

function matchesExpectedSubset(actual, expected) {
	if (Array.isArray(expected)) {
		return Array.isArray(actual) && actual.length === expected.length
			&& expected.every((item, index) => matchesExpectedSubset(actual[index], item));
	}
	if (expected !== null && typeof expected === 'object') {
		if (actual === null || typeof actual !== 'object' || Array.isArray(actual)) return false;
		return Object.entries(expected).every(([key, value]) => Object.hasOwn(actual, key)
			&& matchesExpectedSubset(actual[key], value));
	}
	return Object.is(actual, expected);
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
