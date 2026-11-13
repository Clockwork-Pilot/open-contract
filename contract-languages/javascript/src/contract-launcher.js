import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../');

// This only serializes the runner's manifest decision. Backend behavior lives in bin/contract-isolate.
export function contractLauncherCommand(backend, packageRoot, manifest, command, { read = [], write = [], runtimeProfile = undefined, guestRoot = undefined } = {}) {
	const launcher = path.join(repoRoot, 'bin/contract-isolate');
	const args = [launcher, '--backend', backend, '--package-root', packageRoot];
	for (const name of (read.length ? read : manifest.filesystem.read)) args.push('--read', name);
	for (const name of (write.length ? write : manifest.filesystem.write)) args.push('--write', name);
	if (runtimeProfile) args.push('--runtime-profile', runtimeProfile);
	if (guestRoot) args.push('--guest-root', guestRoot);
	return [...args, '--', ...command];
}
