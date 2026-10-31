export function readLegacy() {
	return {
		version: 1,
		cases: [{ id: 'adapted', input: { value: 4 }, expected: { doubled: 8 } }],
	};
}

export function evaluate(input) {
	return { doubled: input.value * 2 };
}
