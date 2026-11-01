export function evaluate(input) {
	return { answer: input.value + 1 };
}

let source = '';
for await (const chunk of process.stdin) source += chunk;
process.stdout.write(JSON.stringify(evaluate(JSON.parse(source))));
