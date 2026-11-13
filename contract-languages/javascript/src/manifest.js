import { cp, lstat, mkdtemp, mkdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import YAML from 'yaml';

export async function readManifest(manifestPath) {
	manifestPath = await realpath(manifestPath);
	const value = YAML.parse(await readFile(manifestPath, 'utf8'));
	if (!value || Array.isArray(value) || typeof value !== 'object'
		|| Object.keys(value).sort().join(',') !== 'files,filesystem,name,version') {
		throw new Error('manifest must contain exactly version, name, files, and filesystem');
	}
	if (value.version !== '1' || typeof value.name !== 'string' || !value.name) throw new Error("manifest version must be '1' and name must be non-empty");
	const { files, filesystem } = value;
	if (!files || typeof files !== 'object' || Array.isArray(files)
		|| Object.keys(files).some((key) => !['dataset', 'validator', 'support', 'nested_contracts'].includes(key))
		|| typeof files.dataset !== 'string' || typeof files.validator !== 'string') {
		throw new Error('manifest files must contain dataset, validator, optional support, and nested_contracts');
	}
	for (const key of ['support', 'nested_contracts']) if (files[key] !== undefined && !Array.isArray(files[key])) throw new Error(`manifest files.${key} must be an array`);
	if (!filesystem || typeof filesystem !== 'object' || Array.isArray(filesystem)
		|| Object.keys(filesystem).sort().join(',') !== 'read,write'
		|| !Array.isArray(filesystem.read) || !Array.isArray(filesystem.write)) {
		throw new Error('manifest filesystem must contain read and write arrays');
	}
	const root = path.dirname(manifestPath);
	const listed = [files.dataset, files.validator, ...(files.support ?? []), ...(files.nested_contracts ?? [])];
	for (const name of [...listed, ...filesystem.read, ...filesystem.write]) validatePackagePath(name);
	for (const name of listed) {
		const resolved = await realpath(path.join(root, name));
		if (!isInside(root, resolved) || !(await stat(resolved)).isFile()) throw new Error(`manifest file escapes its package or is not a regular file: ${name}`);
	}
	for (const name of filesystem.read) {
		const resolved = await realpath(path.join(root, name));
		if (!isInside(root, resolved)) throw new Error(`manifest read path escapes its package through a symlink: ${name}`);
	}
	for (const name of files.support ?? []) {
		if (!filesystem.read.some((allowed) => name === allowed || name.startsWith(`${allowed}/`))) throw new Error(`support file is not readable under filesystem.read: ${name}`);
	}
	for (const name of [files.dataset, files.validator, ...(files.nested_contracts ?? [])]) {
		if (!filesystem.read.some((allowed) => name === allowed || name.startsWith(`${allowed}/`))) throw new Error(`contract file is not readable under filesystem.read: ${name}`);
	}
	for (const name of filesystem.write) {
		try {
			const resolved = await realpath(path.join(root, name));
			if (!isInside(root, resolved)) throw new Error(`manifest write path escapes its package through a symlink: ${name}`);
		} catch (error) { if (error.code !== 'ENOENT') throw error; }
	}
	return { manifest: value, root };
}

export async function stagePackage(manifest, sourceRoot) {
	const root = await mkdtemp(path.join(os.tmpdir(), 'open-contract-'));
	const names = [manifest.files.dataset, manifest.files.validator, ...(manifest.files.support ?? []), ...(manifest.files.nested_contracts ?? [])];
	try {
		for (const name of names) {
			const source = await realpath(path.join(sourceRoot, name));
			if (!isInside(sourceRoot, source)) throw new Error(`manifest path escapes its package through a symlink: ${name}`);
			const destination = path.join(root, name);
			await mkdir(path.dirname(destination), { recursive: true });
			await cp(source, destination);
		}
		for (const name of manifest.filesystem.write) {
			const destination = path.join(root, name);
			try { await lstat(destination); }
			catch (error) {
				if (error.code !== 'ENOENT') throw error;
				if (path.posix.extname(name)) {
					await mkdir(path.dirname(destination), { recursive: true });
					await writeFile(destination, '');
				} else await mkdir(destination, { recursive: true });
			}
		}
		return root;
	} catch (error) {
		const { rm } = await import('node:fs/promises');
		await rm(root, { recursive: true, force: true });
		throw error;
	}
}

export async function stageContractTree(manifest, sourceRoot, topManifestPath) {
	const root = await mkdtemp(path.join(os.tmpdir(), 'open-contract-tree-'));
	const read = new Set();
	const write = new Set();
	const sourceMap = new Map();
	async function copyTree(current, currentRoot, currentManifestPath, prefix = '', stack = new Set()) {
		currentManifestPath = await realpath(currentManifestPath);
		if (stack.has(currentManifestPath)) throw new Error(`nested contract cycle detected at ${currentManifestPath}`);
		const nextStack = new Set(stack).add(currentManifestPath);
		const files = current.files;
		for (const name of [files.dataset, files.validator, ...(files.support ?? [])]) {
			const source = await realpath(path.join(currentRoot, name));
			if (!isInside(currentRoot, source)) throw new Error(`manifest path escapes its package through a symlink: ${name}`);
			const destination = path.join(root, prefix, name);
			await mkdir(path.dirname(destination), { recursive: true });
			await cp(source, destination);
			sourceMap.set(destination, source);
		}
		for (const name of current.filesystem.read) read.add(path.posix.join(prefix, name));
		for (const name of current.filesystem.write) {
			const mapped = path.posix.join(prefix, name);
			write.add(mapped);
			const destination = path.join(root, mapped);
			try { await lstat(destination); }
			catch (error) {
				if (error.code !== 'ENOENT') throw error;
				if (path.posix.extname(mapped)) { await mkdir(path.dirname(destination), { recursive: true }); await writeFile(destination, ''); }
				else await mkdir(destination, { recursive: true });
			}
		}
		for (const child of files.nested_contracts ?? []) {
			const childPath = await realpath(path.join(currentRoot, child));
			if (!isInside(currentRoot, childPath)) throw new Error(`nested manifest escapes its package: ${child}`);
			const childManifestPath = path.join(root, prefix, child);
			await mkdir(path.dirname(childManifestPath), { recursive: true });
			await cp(childPath, childManifestPath);
			sourceMap.set(childManifestPath, childPath);
			read.add(path.posix.join(prefix, child));
			const { manifest: childManifest, root: childRoot } = await readManifest(childPath);
			await copyTree(childManifest, childRoot, childPath, path.posix.join(prefix, path.posix.dirname(child)), nextStack);
		}
	}
	try {
		await copyTree(manifest, sourceRoot, topManifestPath);
		const topRelative = path.relative(sourceRoot, topManifestPath);
		await cp(topManifestPath, path.join(root, topRelative));
		read.add(topRelative.split(path.sep).join('/'));
		return { root, read: [...read].sort(), write: [...write].sort(), sourceMap };
	} catch (error) {
		const { rm } = await import('node:fs/promises');
		await rm(root, { recursive: true, force: true });
		throw error;
	}
}

export function validatePackagePath(value) {
	if (typeof value !== 'string' || !value || value.includes('\\') || path.posix.isAbsolute(value)
		|| value.split('/').some((part) => !part || part === '.' || part === '..') || value.split('/')[0].includes(':')) {
		throw new Error(`manifest path must stay within the package: ${String(value)}`);
	}
}

function isInside(root, candidate) {
	const relative = path.relative(root, candidate);
	return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}
