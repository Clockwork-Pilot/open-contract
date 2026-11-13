import { increment } from '../javascript/tests/fixtures/internal-coverage-helper.mjs';

export function evaluate(input) {
	return { answer: increment(input.value) };
}

let source = '';
for await (const chunk of process.stdin) source += chunk;
process.stdout.write(JSON.stringify(evaluate(JSON.parse(source))));
