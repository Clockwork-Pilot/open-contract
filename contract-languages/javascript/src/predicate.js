import { isDeepStrictEqual } from 'node:util';
import { LosslessNumber, isLosslessNumber, parse as parseLossless } from 'lossless-json';

const MISSING = Symbol('missing');
const INVALID = Symbol('invalid');
const DIGEST = /^(?:sha256:)?([0-9a-f]{64})$/i;
const TOKEN = /\s*(?:(==|!=|>=|<=|>|<)|([()])|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')|(-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)|((?:\.[A-Za-z0-9_@-]+|\[(?:"(?:\\.|[^"\\])*")\])+)|([A-Za-z_][A-Za-z0-9_]*))/y;

export function evaluatePredicate(expression, record) {
	try {
		const parser = new Parser(expression, record);
		return parser.evaluate();
	} catch {
		return false;
	}
}

class Parser {
	constructor(expression, record) {
		this.record = record;
		this.tokens = tokenize(expression);
		this.index = 0;
	}

	evaluate() {
		if (!this.tokens.length) return false;
		const value = this.parseOr();
		return this.index === this.tokens.length && value === true;
	}

	peek() { return this.tokens[this.index]; }
	take() {
		const token = this.peek();
		if (!token) throw new Error('unexpected end');
		this.index += 1;
		return token;
	}
	word(value) {
		const token = this.peek();
		if (token?.kind === 'word' && token.value.toUpperCase() === value) {
			this.index += 1;
			return true;
		}
		return false;
	}
	parseOr() {
		let value = this.parseAnd();
		while (this.word('OR')) value = this.parseAnd() || value;
		return value;
	}
	parseAnd() {
		let value = this.parseNot();
		while (this.word('AND')) {
			const right = this.parseNot();
			value = value && right;
		}
		return value;
	}
	parseNot() { return this.word('NOT') ? !this.parseNot() : this.parseComparison(); }
	parseComparison() {
		const left = this.parsePrimary();
		const token = this.peek();
		if (token?.kind === 'operator') {
			this.take();
			return compare(resolve(left, this.record), resolve(this.parsePrimary(), this.record), token.value);
		}
		return left === true;
	}
	parsePrimary() {
		const token = this.take();
		if (token.kind === 'paren' && token.value === '(') {
			const value = this.parseOr();
			if (this.take().value !== ')') throw new Error('unclosed parenthesis');
			return value;
		}
		if (token.kind === 'field') return { field: token.value };
		if (token.kind === 'literal') return token.value;
		if (token.kind === 'word') {
			const word = token.value.toUpperCase();
			if (word === 'TRUE') return true;
			if (word === 'FALSE') return false;
			if (word === 'NULL') return null;
			if (word === 'EXISTS') {
				if (this.take().value !== '(') throw new Error('exists requires parentheses');
				const field = this.take();
				if (field.kind !== 'field' || this.take().value !== ')') throw new Error('invalid exists call');
				const value = resolve({ field: field.value }, this.record);
				return value !== MISSING && value !== null && !isDeepStrictEqual(value, {}) && !isDeepStrictEqual(value, []);
			}
		}
		return INVALID;
	}
}

function tokenize(expression) {
	if (typeof expression !== 'string') return [];
	expression = expression.trim();
	const tokens = [];
	let offset = 0;
	while (offset < expression.length) {
		TOKEN.lastIndex = offset;
		const match = TOKEN.exec(expression);
		if (!match) return [];
		offset = TOKEN.lastIndex;
		if (match[1]) tokens.push({ kind: 'operator', value: match[1] });
		else if (match[2]) tokens.push({ kind: 'paren', value: match[2] });
		else if (match[3]) tokens.push({ kind: 'literal', value: decodeString(match[3]) });
		else if (match[4]) tokens.push({ kind: 'literal', value: parseLossless(match[4]) });
		else if (match[5]) tokens.push({ kind: 'field', value: match[5] });
		else if (match[6]) tokens.push({ kind: 'word', value: match[6] });
	}
	return tokens;
}

function decodeString(value) {
	if (value.startsWith('"')) return JSON.parse(value);
	const body = value.slice(1, -1);
	let output = '';
	for (let index = 0; index < body.length; index += 1) {
		if (body[index] !== '\\' || index + 1 >= body.length) {
			output += body[index];
			continue;
		}
		const escaped = body[++index];
		if (escaped === 'u' && /^[0-9a-f]{4}$/i.test(body.slice(index + 1, index + 5))) {
			output += String.fromCharCode(Number.parseInt(body.slice(index + 1, index + 5), 16));
			index += 4;
		} else if (escaped === 'x' && /^[0-9a-f]{2}$/i.test(body.slice(index + 1, index + 3))) {
			output += String.fromCharCode(Number.parseInt(body.slice(index + 1, index + 3), 16));
			index += 2;
		} else {
			output += ({ n: '\n', r: '\r', t: '\t', b: '\b', f: '\f', v: '\v', '0': '\0' })[escaped] ?? escaped;
		}
	}
	return output;
}

function resolve(value, record) {
	if (!value || typeof value !== 'object' || typeof value.field !== 'string') return value;
	let current = record;
	const parts = [];
	const segment = /\.([A-Za-z0-9_@-]+)|\[("(?:\\.|[^"\\])*")\]/g;
	for (const match of value.field.matchAll(segment)) parts.push(match[1] ?? JSON.parse(match[2]));
	for (const part of parts) {
		if (current && typeof current === 'object' && !Array.isArray(current) && Object.hasOwn(current, part)) current = current[part];
		else if (Array.isArray(current) && /^(0|[1-9][0-9]*)$/.test(part) && Number(part) < current.length) current = current[Number(part)];
		else return MISSING;
	}
	return current;
}

function normalized(value) { return value === MISSING ? null : value; }
function equal(left, right) {
	left = normalized(left);
	right = normalized(right);
	if (numeric(left) && numeric(right)) return compareNumbers(left, right) === 0;
	if (typeof left !== typeof right) return false;
	if (typeof left === 'string' && DIGEST.test(left) && DIGEST.test(right)) {
		return left.replace(/^sha256:/i, '').toLowerCase() === right.replace(/^sha256:/i, '').toLowerCase();
	}
	return isDeepStrictEqual(left, right);
}
function category(value) {
	if (value === MISSING || value === null) return null;
	if (typeof value === 'boolean') return 'boolean';
	if (typeof value === 'number' || numeric(value)) return 'number';
	if (typeof value === 'string') return 'string';
	return null;
}
function compare(left, right, operator) {
	if (left === INVALID || right === INVALID) return false;
	if (operator === '==') return equal(left, right);
	if (operator === '!=') return !equal(left, right);
	const leftCategory = category(left);
	const rightCategory = category(right);
	if (!leftCategory || leftCategory !== rightCategory) return false;
	const a = normalized(left);
	const b = normalized(right);
	const orderA = typeof a === 'string' && DIGEST.test(a) ? a.replace(/^sha256:/i, '').toLowerCase() : a;
	const orderB = typeof b === 'string' && DIGEST.test(b) ? b.replace(/^sha256:/i, '').toLowerCase() : b;
	const ordering = leftCategory === 'number' ? compareNumbers(a, b) : orderA < orderB ? -1 : orderA > orderB ? 1 : 0;
	return ({
		'>': ordering > 0, '>=': ordering >= 0, '<': ordering < 0, '<=': ordering <= 0,
	})[operator] ?? false;
}

function numeric(value) { return typeof value === 'number' || value instanceof LosslessNumber || isLosslessNumber(value); }
function compareNumbers(left, right) {
	const parts = (value) => {
		const text = numeric(value) ? value.toString() : String(value);
		const match = /^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(text);
		if (!match) return null;
		const fraction = match[3] ?? '';
		let exponent = Number(match[4] ?? 0) - fraction.length;
		let digits = BigInt(`${match[1]}${match[2]}${fraction}`);
		if (digits === 0n) return { digits: 0n, exponent: 0 };
		while (digits % 10n === 0n) { digits /= 10n; exponent += 1; }
		return { digits, exponent };
	};
	const a = parts(left); const b = parts(right);
	if (!a || !b) return Number(left) < Number(right) ? -1 : Number(left) > Number(right) ? 1 : 0;
	const exponent = Math.min(a.exponent, b.exponent);
	const x = a.digits * (10n ** BigInt(a.exponent - exponent));
	const y = b.digits * (10n ** BigInt(b.exponent - exponent));
	return x < y ? -1 : x > y ? 1 : 0;
}
