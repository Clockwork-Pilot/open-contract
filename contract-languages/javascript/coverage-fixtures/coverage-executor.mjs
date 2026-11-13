export function evaluate(input) {
	return input.branch === 'alpha' ? alpha() : beta();
}

function alpha() {
	const value = 'A';
	return { branch: 'alpha', value };
}

function beta() {
	const value = 'B';
	return { branch: 'beta', value };
}
