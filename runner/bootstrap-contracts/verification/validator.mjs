export function validate(value) {
	if (!Number.isFinite(value?.value)) throw new TypeError('value must be a finite number');
	return { value: value.value + 1 };
}
