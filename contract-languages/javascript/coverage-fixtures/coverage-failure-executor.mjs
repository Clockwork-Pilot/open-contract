export function evaluate(input) {
	if (input.branch === 'throws') throw new Error('fixture executor failure');
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
