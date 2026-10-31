let requests = 0;

export function evaluate(input) {
	requests += 1;
	if (input.operation === 'double') return { value: input.value * 2, requests };
	if (input.operation === 'negate') return { value: -input.value, requests };
	return { error: 'unsupported operation', requests };
}

if (process.argv[1] && new URL(import.meta.url).pathname === process.argv[1]) {
	let text = '';
	for await (const chunk of process.stdin) text += chunk;
	process.stdout.write(JSON.stringify(evaluate(JSON.parse(text))));
}
