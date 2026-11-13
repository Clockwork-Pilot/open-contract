#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { accessSync } from 'node:fs';

async function runValidator(validatorPath, root, input) {
	const extension = path.extname(validatorPath).toLowerCase();
	let command;
	if (extension === '.js' || extension === '.mjs') {
		const code = "const m=await import((await import('node:url')).pathToFileURL(process.argv[1])); const f=m.validate??m.default; if(typeof f!=='function')throw Error('validator must export validate(input) or default'); let s=''; for await(const c of process.stdin)s+=c; console.log(JSON.stringify(await f(JSON.parse(s))));";
		command = [process.execPath, '--input-type=module', '-e', code, validatorPath];
	} else if (extension === '.py') {
		const python = resolveCommand('python3');
		const code = "import importlib.util,json,sys; root,source=sys.argv[1:3]; sys.path.insert(0,root); spec=importlib.util.spec_from_file_location('open_contract_validator',source); m=importlib.util.module_from_spec(spec); sys.modules[spec.name]=m; spec.loader.exec_module(m); print(json.dumps(m.validate(json.load(sys.stdin))))";
		command = [python, '-c', code, root, validatorPath];
	} else throw new Error(`unsupported validator file type: ${extension}`);
	const { stdout, stderr, code, signal } = await spawnInput(command, JSON.stringify(input), { cwd: root, env: process.env });
	if (code !== 0) throw new Error(`validator ${signal ? `signal ${signal}` : `exit ${code}`}: ${stderr.trim()}`);
	try { return JSON.parse(stdout); } catch { throw new Error('validator must print one JSON value to stdout'); }
}

function spawnInput(command, input, options) {
	return new Promise((resolve, reject) => {
		const child = spawn(command[0], command.slice(1), { ...options, stdio: ['pipe', 'pipe', 'pipe'] });
		let stdout = ''; let stderr = '';
		child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk; });
		child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk; });
		child.on('error', reject);
		child.on('close', (code, signal) => resolve({ stdout, stderr, code, signal }));
		child.stdin.end(input);
	});
}

function resolveCommand(command) {
	if (command.includes(path.sep)) return path.resolve(command);
	for (const directory of (process.env.PATH ?? '').split(path.delimiter)) {
		const candidate = path.join(directory, command);
		try { accessSync(candidate); return candidate; } catch { /* continue searching PATH */ }
	}
	throw new Error(`runtime command not found: ${command}`);
}

const root = process.argv[2];
if (!root) throw new Error('execution adapter requires the staged package root');
const packageRoot = path.resolve(root);
for await (const line of createInterface({ input: process.stdin })) {
	try {
		const request = JSON.parse(line);
		const validator = path.resolve(packageRoot, request.validator);
		if (validator !== packageRoot && !validator.startsWith(`${packageRoot}${path.sep}`)) throw new Error('validator escaped package root');
		const output = await runValidator(validator, packageRoot, request.input);
		process.stdout.write(`${JSON.stringify({ output })}\n`);
	} catch (error) {
		process.stdout.write(`${JSON.stringify({ error: `${error.name}: ${error.message}` })}\n`);
	}
}
