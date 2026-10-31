import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { parse } from 'lossless-json';
import { evaluatePredicate } from '../src/predicate.js';

const corpus = parse(await readFile(new URL('../../../bootstrap-contracts/predicates/dataset.json', import.meta.url), 'utf8'));

test('matches every shared predicate language case', () => {
	for (const item of corpus.cases) {
		assert.equal(evaluatePredicate(item.predicate, item.json), item.expected, item.id);
	}
});

test('evaluates the right side of false AND expressions', () => {
	assert.equal(evaluatePredicate('false AND .value == 1', { value: 1 }), false);
	assert.equal(evaluatePredicate('true OR .value == 1', { value: 1 }), true);
});
