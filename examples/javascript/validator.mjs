export function validate(input) {
	if (typeof input.name !== 'string') {
		throw new TypeError('name must be a string');
	}
	return { greeting: `hello, ${input.name}` };
}
