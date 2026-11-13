#!/usr/bin/env python3
"""Run a contract command inside its declared filesystem boundary."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import yaml


OPEN_CONTRACT_ROOT = Path(__file__).resolve().parents[1]
LANDLOCK_RUNNER = OPEN_CONTRACT_ROOT / "bin" / "landlock-run"
BWRAP_RUNNER = OPEN_CONTRACT_ROOT / "bin" / "bwrap-run"
SYSTEM_READS = ["/usr", "/bin", "/lib", "/lib64", "/etc/ssl", "/dev", "/proc"]
BWRAP_SYSTEM_BINDS = [Path("/usr"), Path("/bin"), Path("/lib"), Path("/lib64"), Path("/etc/ssl")]
JAVASCRIPT_RUNNER = OPEN_CONTRACT_ROOT / "contract-languages" / "javascript"
JAVASCRIPT_RUNNER_CLI = JAVASCRIPT_RUNNER / "src" / "cli.js"
JAVASCRIPT_COVERAGE_COMMAND = JAVASCRIPT_RUNNER / "node_modules" / ".bin" / "c8"
RUNNER_DEPENDENCIES = {
	"javascript": [
		JAVASCRIPT_RUNNER,
		OPEN_CONTRACT_ROOT / "schemas",
		OPEN_CONTRACT_ROOT / "runner/bootstrap-contracts" / "predicates" / "predicate-language.schema.json",
	],
}


def parse_args(argv):
	if argv[:1] == ["run"]:
		argv = argv[1:]
	if "--" in argv:
		separator = argv.index("--")
		command = argv[separator + 1:]
		argv = argv[:separator]
	else:
		command = []
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("target", nargs="?", help="contract directory or path to its open-contract.yaml manifest")
	parser.add_argument("--contract-dir", help="legacy spelling for the contract directory")
	parser.add_argument("--config", help="manifest path relative to the contract directory (default: open-contract.yaml)")
	parser.add_argument("--dataset", help="dataset path relative to the contract directory; overrides only this manifest")
	parser.add_argument("--isolation", choices=("auto", "bubblewrap", "landlock", "none"))
	parser.add_argument("--read", action="append", default=[], metavar="PATH", help="additional read-only dependency path")
	parser.add_argument("--write", action="append", default=[], metavar="PATH", help="additional writable output path")
	parser.add_argument("--env", action="append", default=[], metavar="NAME", help="pass this host environment variable to the command")
	parser.add_argument("--network", action="store_true", default=None, help="allow network access when using Bubblewrap")
	args = parser.parse_args(argv)
	args.command = command
	return args


def fail(message):
	print(f"open-contract run: {message}", file=sys.stderr)
	return 2


def existing_path(value, *, directory=None):
	path = Path(value).expanduser().resolve(strict=True)
	if directory is True and not path.is_dir():
		raise ValueError(f"expected a directory: {value}")
	if directory is False and not path.is_file():
		raise ValueError(f"expected a file: {value}")
	return path


def resolve_contract(args):
	if args.target and args.contract_dir:
		raise ValueError("provide a contract directory or a manifest path, not both")
	target_value = args.target or args.contract_dir
	if not target_value:
		raise ValueError("provide a contract directory or a path to open-contract.yaml")
	target = existing_path(target_value)
	if target.is_dir():
		return target, args.config or "open-contract.yaml"
	if args.config:
		raise ValueError("--config cannot be combined with a manifest path")
	if target.name != "open-contract.yaml":
		raise ValueError(f"expected a contract directory or open-contract.yaml manifest: {target}")
	return target.parent, target


def inside(path, root):
	try:
		path.relative_to(root)
		return True
	except ValueError:
		return False


def overlaps(path, other):
	return inside(path, other) or inside(other, path)


def collect_paths(args, contract_dir, dataset):
	reads = [Path(path) for path in SYSTEM_READS if Path(path).exists()]
	child_dirs = [Path(path).resolve() for path in getattr(args, "child_contract_directories", [])]
	if child_dirs:
		reads.extend(owned_contract_files(contract_dir, child_dirs))
	else:
		reads.append(contract_dir)
	reads.append(dataset)
	for value in args.read:
		path = Path(expand_root(value)).expanduser()
		resolved = existing_path(path if path.is_absolute() else contract_dir / path)
		if any(overlaps(resolved, child) for child in child_dirs):
			raise ValueError(f"parent contract cannot read a child contract resource: {value}")
		if args.enforce_manifest_ownership and not inside(resolved, contract_dir) and value in args.manifest_read:
			raise ValueError(f"manifest read path must stay inside the contract directory: {value}")
		reads.append(resolved)
	for value in args.write:
		path = Path(expand_root(value)).expanduser()
		resolved = existing_path(path if path.is_absolute() else contract_dir / path)
		if any(overlaps(resolved, child) for child in child_dirs):
			raise ValueError(f"parent contract cannot write a child contract resource: {value}")
		if args.enforce_manifest_ownership and not inside(resolved, contract_dir) and value in args.manifest_write:
			raise ValueError(f"manifest write path must stay inside the contract directory: {value}")
	writes = [Path(expand_root(value)).expanduser() for value in args.write]
	writes = [path.resolve(strict=True) if path.is_absolute() else (contract_dir / path).resolve(strict=True) for path in writes]
	return unique(reads), writes


def owned_contract_files(contract_dir, child_dirs):
	"""Return the parent's files without exposing any explicitly nested child tree."""
	contract_dir = Path(contract_dir).resolve()
	children = [Path(path).resolve() for path in child_dirs]
	owned = []
	for root, directories, files in os.walk(contract_dir, followlinks=False):
		root_path = Path(root).resolve()
		directories[:] = [name for name in directories if not any(
			inside((root_path / name).resolve(), child) for child in children
		)]
		if root_path != contract_dir and not any(inside(child, root_path) for child in children):
			owned.append(root_path)
		for name in files:
			path = root_path / name
			resolved = path.resolve()
			if not inside(resolved, contract_dir) or any(inside(resolved, child) for child in children):
				continue
			if resolved.is_file():
				owned.append(resolved)
	return owned


def expand_root(value):
	return value.replace("${OPEN_CONTRACT_ROOT}", str(OPEN_CONTRACT_ROOT)).replace(
		"$OPEN_CONTRACT_ROOT", str(OPEN_CONTRACT_ROOT)
	)


def manifest_string_list(manifest, name):
	value = manifest.get(name, [])
	if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
		raise ValueError(f"manifest {name} must be an array of non-empty strings")
	return value


def child_contract_directories(manifest, contract_dir):
	values = manifest_string_list(manifest, "contracts")
	if "contracts" in manifest and not values:
		raise ValueError("manifest contracts must contain at least one child directory")
	children = []
	seen = set()
	for value in values:
		path = Path(value).expanduser()
		if path.is_absolute() or "$OPEN_CONTRACT_ROOT" in value or "${OPEN_CONTRACT_ROOT}" in value:
			raise ValueError(f"manifest contracts paths must be relative to the contract directory: {value}")
		child = existing_path(contract_dir / path, directory=True)
		if child == contract_dir or not inside(child, contract_dir):
			raise ValueError(f"child contract must stay inside the parent contract directory: {value}")
		if child in seen:
			raise ValueError(f"manifest contracts contains a duplicate child directory: {value}")
		seen.add(child)
		if not (child / "open-contract.yaml").is_file():
			raise ValueError(f"child contract manifest does not exist: {child / 'open-contract.yaml'}")
		children.append(child)
	return children


def load_manifest(args, contract_dir, selected_config):
	config_value = Path(selected_config)
	config_path = (config_value if config_value.is_absolute() else contract_dir / config_value).resolve(strict=False)
	if not inside(config_path, contract_dir):
		raise ValueError("manifest must be inside the contract directory")
	if not config_path.exists():
		if args.config:
			raise ValueError(f"manifest does not exist: {config_path}")
		if args.dataset and args.command:
			return {"version": 1, "dataset": args.dataset, "isolation": "auto", "read": [], "write": [], "env": [], "network": False, "command": []}
		raise ValueError(f"manifest not found: {config_path}")
	if not config_path.is_file():
		raise ValueError(f"manifest is not a file: {config_path}")
	manifest = yaml.safe_load(config_path.read_text(encoding="utf-8"))
	if not isinstance(manifest, dict) or manifest.get("version") != 1:
		raise ValueError("manifest version must be 1")
	dataset = manifest.get("dataset")
	if dataset is not None and (not isinstance(dataset, str) or not dataset):
		raise ValueError("manifest dataset must be a path")
	if dataset is not None:
		dataset_path = Path(dataset).expanduser()
		if dataset_path.is_absolute() or "$OPEN_CONTRACT_ROOT" in dataset or "${OPEN_CONTRACT_ROOT}" in dataset:
			raise ValueError(f"manifest dataset path must be relative to the contract directory: {dataset}")
		if not inside((contract_dir / dataset_path).resolve(strict=False), contract_dir):
			raise ValueError(f"manifest dataset must stay inside the contract directory: {dataset}")
	contracts = child_contract_directories(manifest, contract_dir)
	if dataset is not None:
		resolved_dataset = (contract_dir / dataset).resolve(strict=False)
		if any(inside(resolved_dataset, child) for child in contracts):
			raise ValueError(f"manifest dataset must not be inside a child contract: {dataset}")
		if args.dataset:
			override_dataset = Path(args.dataset).expanduser()
			override_dataset = override_dataset.resolve(strict=False) if override_dataset.is_absolute() else (contract_dir / override_dataset).resolve(strict=False)
			if any(inside(override_dataset, child) for child in contracts):
				raise ValueError(f"dataset override must not be inside a child contract: {args.dataset}")
	if dataset is None and not contracts:
		raise ValueError("manifest must declare a dataset or child contracts")
	if dataset is None:
		for name in ("runner", "isolation", "command", "read", "write", "env", "network"):
			if name in manifest:
				raise ValueError(f"manifest {name} requires a dataset")
	if manifest.get("isolation", "auto") not in ("auto", "bubblewrap", "landlock", "none"):
		raise ValueError("manifest isolation must be auto, bubblewrap, landlock, or none")
	command = manifest_string_list(manifest, "command")
	if dataset is not None and not command:
		raise ValueError("manifest command must not be empty")
	if dataset is None and command:
		raise ValueError("manifest command requires a dataset")
	runner = manifest.get("runner")
	if runner is not None and (not isinstance(runner, str) or runner not in RUNNER_DEPENDENCIES):
		raise ValueError(f"unsupported manifest runner: {runner}")
	if dataset is None and runner is not None:
		raise ValueError("manifest runner requires a dataset")
	manifest["read"] = manifest_string_list(manifest, "read")
	manifest["write"] = manifest_string_list(manifest, "write")
	manifest["env"] = manifest_string_list(manifest, "env")
	if runner == "javascript":
		if any("$OPEN_CONTRACT_ROOT" in item or "${OPEN_CONTRACT_ROOT}" in item for item in command):
			raise ValueError("JavaScript runner arguments must not name Open Contract's internal files")
		if "--coverage-command" in command:
			raise ValueError("Open Contract supplies the JavaScript coverage command")
	for name in ("read", "write"):
		for value in manifest[name]:
			path = Path(value).expanduser()
			if path.is_absolute() or "$OPEN_CONTRACT_ROOT" in value or "${OPEN_CONTRACT_ROOT}" in value:
				raise ValueError(f"manifest {name} paths must be relative to the contract directory: {value}")
			resolved = (contract_dir / path).resolve(strict=True)
			if not inside(resolved, contract_dir):
				raise ValueError(f"manifest {name} path must stay inside the contract directory: {value}")
			if any(overlaps(resolved, child) for child in contracts):
				raise ValueError(f"manifest {name} path must not include a child contract: {value}")
	if "network" in manifest and not isinstance(manifest["network"], bool):
		raise ValueError("manifest network must be a boolean")
	return manifest


def javascript_application_package(contract_dir):
	package_root = None
	package = None
	for parent in contract_dir.parents:
		package_path = parent / "package.json"
		if not package_path.is_file():
			continue
		try:
			candidate = json.loads(package_path.read_text(encoding="utf-8"))
		except (OSError, json.JSONDecodeError):
			continue
		if any(key.startswith("#contract/") for key in candidate.get("imports", {})):
			package_root, package = parent, candidate
			break
	return package_root, package


def javascript_application_coverage_sources(contract_dir, excluded_contracts=()):
	"""Resolve host-application coverage inputs declared outside the contract manifest."""
	package_root, package = javascript_application_package(contract_dir)
	if package_root is None:
		return []
	contract_key = contract_dir.relative_to(package_root).as_posix()
	runner_config = package.get("openContractRunner", {})
	if not isinstance(runner_config, dict):
		raise ValueError("package.json openContractRunner must be an object")
	contracts = runner_config.get("contracts", {})
	if not isinstance(contracts, dict):
		raise ValueError("package.json openContractRunner.contracts must be an object")
	contract_config = contracts.get(contract_key, {})
	if not isinstance(contract_config, dict):
		raise ValueError(f"package.json openContractRunner entry must be an object: {contract_key}")
	sources = contract_config.get("coverageSources", [])
	if not isinstance(sources, list) or any(not isinstance(source, str) or not source for source in sources):
		raise ValueError(f"package.json coverageSources must be an array of paths: {contract_key}")
	resolved = []
	for source in sources:
		path = Path(source).expanduser()
		if path.is_absolute():
			raise ValueError(f"package.json coverageSources must be relative to the application package: {source}")
		coverage_source = existing_path(package_root / path, directory=False)
		if any(inside(coverage_source, Path(child).resolve()) for child in excluded_contracts):
			raise ValueError(f"application coverage source must not include a child contract: {source}")
		resolved.append(coverage_source)
	return resolved


def javascript_application_dependencies(contract_dir, excluded_contracts=()):
	"""Mount only application files imported by this contract and its schema ancestors."""
	package_root, package = javascript_application_package(contract_dir)
	if package_root is None:
		return []
	excluded_contracts = [Path(path).resolve() for path in excluded_contracts]
	def is_excluded(path):
		resolved = Path(path).resolve()
		return any(inside(resolved, child) for child in excluded_contracts)

	dependencies = {package_root / "package.json"}
	contract_key = contract_dir.relative_to(package_root).as_posix()
	contract_settings = package.get("openContractRunner", {}).get("contracts", {}).get(contract_key, {})
	runner_dependencies = contract_settings.get("runnerDependencies", [])
	if not isinstance(runner_dependencies, list) or any(not isinstance(value, str) or not value for value in runner_dependencies):
		raise ValueError(f"package.json openContractRunner.contracts.{contract_key}.runnerDependencies must be an array of paths")
	runner_modules = []
	for value in runner_dependencies:
		path = Path(value).expanduser()
		if path.is_absolute():
			raise ValueError(f"runner dependency paths must be relative to the application package: {value}")
		resolved = (package_root / path).resolve(strict=True)
		dependencies.add(resolved)
		if resolved.is_dir():
			runner_modules.extend(item for item in resolved.rglob("*")
				if item.is_file() and item.suffix in (".js", ".mjs", ".cjs"))
		elif resolved.suffix in (".js", ".mjs", ".cjs"):
			runner_modules.append(resolved)
	for parent in contract_dir.parents:
		if parent == package_root / "contracts":
			break
		schema = parent / "schema"
		if schema.is_dir():
			dependencies.add(schema)

	imports_pattern = re.compile(r"(?:from\s*|import\s*\()\s*['\"]([^'\"]+)['\"]")
	url_pattern = re.compile(r"new\s+URL\(\s*['\"]([^'\"]+)['\"]\s*,\s*import\.meta\.url\s*\)")
	modules = [*runner_modules, *(path for path in contract_dir.rglob("*") if path.is_file() and path.resolve().is_relative_to(contract_dir)
		and not is_excluded(path)
		and path.suffix in (".js", ".mjs", ".cjs"))]
	visited = set()
	needs_node_modules = False
	shared_dependency = package.get("dependencies", {}).get("@license-server/shared")
	shared_root = None
	if isinstance(shared_dependency, str) and shared_dependency.startswith("file:"):
		shared_root = (package_root / shared_dependency[5:]).resolve()

	while modules:
		module = modules.pop().resolve()
		if module in visited:
			continue
		visited.add(module)
		try:
			contents = module.read_text(encoding="utf-8")
		except (OSError, UnicodeDecodeError):
			continue
		for specifier in imports_pattern.findall(contents):
			target = None
			if specifier.startswith("#contract/"):
				for key, value in package.get("imports", {}).items():
					if key.endswith("*") and specifier.startswith(key[:-1]):
						target = package_root / value.replace("*", specifier[len(key) - 1:])
						break
			elif specifier.startswith("."):
				target = (module.parent / specifier).resolve()
				allowed_root = contract_dir if module.is_relative_to(contract_dir) else package_root
				if target.is_file() and target.is_relative_to(allowed_root) and not is_excluded(target):
					dependencies.add(target)
					modules.append(target)
				continue
			elif specifier.startswith("@license-server/shared/"):
				needs_node_modules = True
				if shared_root is not None:
					dependencies.add(shared_root)
				continue
			elif specifier.startswith("node:"):
				continue
			else:
				needs_node_modules = True
				continue
			if target is not None:
				target = target.resolve()
				if target.is_file() and target.is_relative_to(package_root) and not is_excluded(target):
					dependencies.add(target)
					modules.append(target)
		for resource in url_pattern.findall(contents):
			target = (module.parent / resource).resolve()
			if (target.is_file() and target.is_relative_to(package_root)
					and not is_excluded(target)
					and (not module.is_relative_to(contract_dir) or not target.is_relative_to(package_root / "contracts"))):
				dependencies.add(target)
				if target.suffix in (".js", ".mjs", ".cjs"):
					modules.append(target)
	if needs_node_modules and (package_root / "node_modules").is_dir():
		dependencies.add(package_root / "node_modules")
	return sorted(dependencies)


def prepare_command(manifest, override, contract_dir, child_contract_directories=()):
	if override:
		return override, []
	runner = manifest.get("runner")
	command = list(manifest.get("command", []))
	if runner == "javascript":
		command = ["node", str(JAVASCRIPT_RUNNER_CLI), *command]
		if "--coverage-threshold" in command and "--coverage-command" not in command:
			command.extend(["--coverage-command", str(JAVASCRIPT_COVERAGE_COMMAND)])
		coverage_sources = javascript_application_coverage_sources(contract_dir, child_contract_directories)
		for source in coverage_sources:
			command.extend(["--coverage-source", str(source)])
		if coverage_sources:
			command.append("--coverage-allow-external")
		return command, [*RUNNER_DEPENDENCIES[runner],
			*javascript_application_dependencies(contract_dir, child_contract_directories), *coverage_sources]
	return command, []


def unique(paths):
	result = []
	seen = set()
	for path in paths:
		path = Path(path).resolve(strict=True)
		if path not in seen:
			seen.add(path)
			result.append(path)
	return result


def command_path(command):
	value = command[0]
	resolved = shutil.which(value) if os.sep not in value else value
	if resolved is None:
		raise ValueError(f"runner command not found: {value}")
	return Path(resolved).resolve(strict=True)


def bind_mount_args(reads, writes, created, mounted_roots):
	args = []

	def ensure_directory(directory):
		missing = []
		current = directory
		while current not in created and current != current.parent:
			missing.append(current)
			current = current.parent
		for item in reversed(missing):
			args.extend(["--dir", str(item)])
			created.add(item)

	for source in reads:
		if source == Path("/dev") or source == Path("/proc"):
			continue
		if any(inside(source, mounted) for mounted in mounted_roots):
			continue
		ensure_directory(source.parent)
		if source.is_dir():
			ensure_directory(source)
		args.extend(["--ro-bind", str(source), str(source)])
		created.add(source)
		mounted_roots.add(source)
	for source in writes:
		if not any(inside(source, mounted) for mounted in mounted_roots):
			ensure_directory(source.parent)
		if source.is_dir():
			if not any(inside(source, mounted) for mounted in mounted_roots):
				ensure_directory(source)
		args.extend(["--bind", str(source), str(source)])
		created.add(source)
	return args


def bwrap_args(args, reads, writes, contract_dir, dataset, executable):
	flags = []
	created = {Path("/")}
	mounted_roots = set()
	for source in BWRAP_SYSTEM_BINDS:
		path = Path(source)
		if not path.exists():
			continue
		for parent in reversed(list(path.parents)[:-1]):
			if parent not in created:
				flags.extend(["--dir", str(parent)])
				created.add(parent)
		if path not in created:
			if path.is_dir():
				flags.extend(["--dir", str(path)])
			flags.extend(["--ro-bind-try", str(path), str(path)])
			created.add(path)
			mounted_roots.add(path)
	flags.extend(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--dir", "/tmp/open-contract"])
	created.update({Path("/dev"), Path("/proc"), Path("/tmp"), Path("/tmp/open-contract")})
	mounted_roots.update({Path("/dev"), Path("/proc"), Path("/tmp")})
	flags.extend(bind_mount_args(reads, writes, created, mounted_roots))
	flags.extend(["--chdir", str(contract_dir)])
	flags.extend(["--setenv", "PATH", path_value(reads, executable)])
	flags.extend(["--setenv", "HOME", "/tmp/open-contract", "--setenv", "TMPDIR", "/tmp"])
	flags.extend(["--setenv", "OPEN_CONTRACT_DIR", str(contract_dir)])
	flags.extend(["--setenv", "OPEN_CONTRACT_DATASET", str(dataset)])
	flags.extend(["--setenv", "OPEN_CONTRACT_ISOLATION", "bubblewrap"])
	if getattr(args, "coverage_artifact", None):
		flags.extend(["--setenv", "OPEN_CONTRACT_COVERAGE_ARTIFACT", str(args.coverage_artifact)])
	if args.network:
		flags.append("--share-net")
	for name in args.env:
		if name in os.environ:
			flags.extend(["--setenv", name, os.environ[name]])
	return flags


def path_value(reads, executable=None):
	directories = []
	if executable is not None:
		directories.append(str(Path(executable).resolve().parent))
	directories.extend(["/usr/local/bin", "/usr/bin", "/bin"])
	directories.extend(str(path if path.is_dir() else path.parent) for path in reads)
	return os.pathsep.join(dict.fromkeys(directories))


def launch_bubblewrap(args, reads, writes, contract_dir, dataset, executable, command):
	if not BWRAP_RUNNER.is_file():
		return False, subprocess.CompletedProcess([], 127, stderr="Bubblewrap launcher is missing")
	flags = bwrap_args(args, reads, writes, contract_dir, dataset, executable)
	probe = subprocess.run(["sh", str(BWRAP_RUNNER), *flags, "--", "/bin/true"], cwd=contract_dir,
		stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
	if probe.returncode != 0:
		return False, probe
	print(f"Open Contract isolation: bubblewrap (contract={contract_dir}; network={'allowed' if args.network else 'denied'})", file=sys.stderr)
	return True, subprocess.run(["sh", str(BWRAP_RUNNER), *flags, "--", *command], cwd=contract_dir)


def landlock_args(reads, writes, command):
	arguments = []
	for path in reads:
		arguments.extend(["--read", str(path)])
	for path in writes:
		arguments.extend(["--write", str(path)])
	arguments.extend(["--", *command])
	return arguments


def launch_landlock(args, reads, writes, contract_dir, dataset, command, temporary):
	if not LANDLOCK_RUNNER.is_file():
		return False, subprocess.CompletedProcess([], 127, stderr="Landlock launcher is missing")
	landlock_reads = [*reads, Path("/dev"), Path("/proc")]
	landlock_writes = [*writes, temporary]
	environment = clean_environment(args, contract_dir, dataset, "landlock", temporary, reads, command[0])
	probe = subprocess.run([sys.executable, str(LANDLOCK_RUNNER), *landlock_args(landlock_reads, landlock_writes, ["/bin/true"])],
		cwd=contract_dir, env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
	if probe.returncode != 0:
		return False, probe
	print(f"Open Contract isolation: landlock (filesystem only; network remains available; contract={contract_dir})", file=sys.stderr)
	return True, subprocess.run([sys.executable, str(LANDLOCK_RUNNER), *landlock_args(landlock_reads, landlock_writes, command)],
		cwd=contract_dir, env=environment)


def clean_environment(args, contract_dir, dataset, backend, temporary, reads, executable=None):
	path_entries = []
	if executable is not None:
		path_entries.append(str(Path(executable).resolve().parent))
	path_entries.extend(["/usr/local/bin", "/usr/bin", "/bin"])
	path_entries.extend(str(path if path.is_dir() else path.parent) for path in reads)
	environment = {
		"PATH": os.pathsep.join(dict.fromkeys(path_entries)),
		"HOME": str(temporary),
		"TMPDIR": str(temporary),
		"OPEN_CONTRACT_DIR": str(contract_dir),
		"OPEN_CONTRACT_DATASET": str(dataset),
		"OPEN_CONTRACT_ISOLATION": backend,
	}
	if getattr(args, "coverage_artifact", None):
		environment["OPEN_CONTRACT_COVERAGE_ARTIFACT"] = str(args.coverage_artifact)
	for name in args.env:
		if name in os.environ:
			environment[name] = os.environ[name]
	return environment


def load_contract_tree(args, contract_dir, selected_config, ancestors=()):
	contract_dir = existing_path(contract_dir, directory=True)
	if contract_dir in ancestors:
		raise ValueError(f"recursive contract composition cycle: {contract_dir}")
	manifest = load_manifest(args, contract_dir, selected_config)
	dataset = None
	if manifest.get("dataset") is not None:
		dataset_value = Path(args.dataset or manifest["dataset"])
		dataset = existing_path(dataset_value if dataset_value.is_absolute() else contract_dir / dataset_value, directory=False)
		if not inside(dataset, contract_dir):
			raise ValueError(f"dataset must be inside the contract directory: {dataset}")
	elif (args.dataset or args.command or getattr(args, "isolation", None) or getattr(args, "read", [])
			or getattr(args, "write", []) or getattr(args, "env", []) or getattr(args, "network", None) is not None):
		raise ValueError("runner overrides require this contract to declare a dataset")

	children = []
	child_args = argparse.Namespace(config=None, dataset=None, command=[])
	for child_dir in child_contract_directories(manifest, contract_dir):
		children.append(load_contract_tree(child_args, child_dir, "open-contract.yaml", (*ancestors, contract_dir)))
	return {"directory": contract_dir, "manifest": manifest, "dataset": dataset, "children": children}


def run_contract_dataset(base_args, contract):
	contract_dir = contract["directory"]
	manifest = contract["manifest"]
	dataset = contract["dataset"]
	args = argparse.Namespace(**vars(base_args))
	args.read = list(base_args.read)
	args.write = list(base_args.write)
	args.env = list(base_args.env)
	args.command = list(base_args.command)
	if contract.get("nested"):
		args.dataset = None
		args.isolation = None
		args.read = []
		args.write = []
		args.env = []
		args.network = None
		args.command = []
	args.isolation = args.isolation or manifest.get("isolation", "auto")
	args.manifest_read = list(manifest.get("read", []))
	args.manifest_write = list(manifest.get("write", []))
	args.enforce_manifest_ownership = True
	args.read = [*manifest.get("read", []), *args.read]
	args.write = [*manifest.get("write", []), *args.write]
	args.env = [*manifest.get("env", []), *args.env]
	args.network = args.network if args.network is not None else manifest.get("network", False)
	args.child_contract_directories = [child["directory"] for child in contract["children"]]
	args.coverage_artifact = None
	if getattr(base_args, "coverage_root", None) is not None:
		contract_key = contract["directory"].relative_to(base_args.root_directory).as_posix().replace("/", "__")
		artifact_dir = base_args.coverage_root / contract_key
		artifact_dir.mkdir(parents=True, exist_ok=True)
		args.coverage_artifact = artifact_dir / "coverage-final.json"
		args.write.append(str(artifact_dir))
	try:
		command, runner_reads = prepare_command(manifest, args.command, contract_dir, args.child_contract_directories)
		if not command:
			return fail("a runner command is required after -- or in open-contract.yaml")
		command = [expand_root(argument) for argument in command]
		reads, writes = collect_paths(args, contract_dir, dataset)
		reads = unique([*reads, *runner_reads])
		executable = command_path(command)
		reads = unique([*reads, executable])
	except (OSError, ValueError, yaml.YAMLError) as error:
		return fail(str(error))

	command = [str(executable), *command[1:]]
	if args.isolation == "none":
		print(f"Open Contract isolation: none (explicit; contract={contract_dir})", file=sys.stderr)
		environment = {**os.environ, "OPEN_CONTRACT_DIR": str(contract_dir),
			"OPEN_CONTRACT_DATASET": str(dataset), "OPEN_CONTRACT_ISOLATION": "none"}
		if args.coverage_artifact:
			environment["OPEN_CONTRACT_COVERAGE_ARTIFACT"] = str(args.coverage_artifact)
		return subprocess.run([str(executable), *command[1:]], cwd=contract_dir, env=environment).returncode

	backends = [args.isolation] if args.isolation != "auto" else ["bubblewrap", "landlock"]
	failures = []
	with tempfile.TemporaryDirectory(prefix="open-contract-run-") as temporary_name:
		temporary = Path(temporary_name).resolve()
		for backend in backends:
			if backend == "bubblewrap":
				started, result = launch_bubblewrap(args, reads, writes, contract_dir, dataset, executable, [str(executable), *command[1:]])
			else:
				started, result = launch_landlock(args, reads, writes, contract_dir, dataset, [str(executable), *command[1:]], temporary)
			if started:
				return result.returncode
			failure = result.stderr.strip() if result.stderr else f"probe exited with status {result.returncode}"
			failures.append(f"{backend}: {failure}")
			if args.isolation != "auto":
				return fail(f"requested {backend} isolation failed before the contract command ran: {failure}")

	print(f"Open Contract isolation: none (auto fallback; no backend could start: {'; '.join(failures)})", file=sys.stderr)
	environment = {**os.environ, "OPEN_CONTRACT_DIR": str(contract_dir),
		"OPEN_CONTRACT_DATASET": str(dataset), "OPEN_CONTRACT_ISOLATION": "none"}
	if args.coverage_artifact:
		environment["OPEN_CONTRACT_COVERAGE_ARTIFACT"] = str(args.coverage_artifact)
	return subprocess.run([str(executable), *command[1:]], cwd=contract_dir, env=environment).returncode


def execute_contract_tree(base_args, contract, root_directory, *, root=True):
	status = 0
	if contract["dataset"] is not None:
		contract["nested"] = not root
		status = run_contract_dataset(base_args, contract)
		artifact_dir = base_args.coverage_root / contract["directory"].relative_to(root_directory).as_posix().replace("/", "__")
		contract["coverage_artifact"] = artifact_dir / "coverage-final.json"
		if contract["coverage_artifact"].is_file():
			coverage = load_coverage_artifact(contract["coverage_artifact"])
			metrics = coverage_metrics(coverage)
			print_coverage_metrics(contract["directory"].relative_to(root_directory).as_posix(), metrics)
			if any(metric["pct"] < 100 for metric in metrics.values()):
				status = status or 1

	for child in contract["children"]:
		label = child["directory"].relative_to(root_directory).as_posix()
		print(f"Open Contract child: {label}")
		child["nested"] = True
		child_status = execute_contract_tree(base_args, child, root_directory, root=False)
		print(f"Open Contract child result: {label} (exit status {child_status})")
		if status == 0 and child_status != 0:
			status = child_status
	return status


def load_coverage_artifact(path):
	coverage = json.loads(Path(path).read_text(encoding="utf-8"))
	if not isinstance(coverage, dict) or not coverage:
		raise ValueError(f"coverage artifact must be a non-empty Istanbul coverage map: {path}")
	normalized = {}
	for source, record in coverage.items():
		if not isinstance(record, dict) or not all(key in record for key in ("statementMap", "fnMap", "branchMap", "s", "f", "b")):
			raise ValueError(f"unsupported coverage artifact for {source}: expected Istanbul coverage-map fields")
		for map_name, counter_name in (("statementMap", "s"), ("fnMap", "f"), ("branchMap", "b")):
			if not isinstance(record[map_name], dict) or not isinstance(record[counter_name], dict):
				raise ValueError(f"invalid {map_name} or {counter_name} map for {source}")
			if record[map_name].keys() != record[counter_name].keys():
				raise ValueError(f"incompatible {map_name} and {counter_name} counters for {source}")
		for branch, locations in record["branchMap"].items():
			if len(locations.get("locations", [])) != len(record["b"][branch]):
				raise ValueError(f"incompatible branch shape for {source}")
		for counter_name in ("s", "f"):
			if any(not isinstance(value, (int, float)) or value < 0 for value in record[counter_name].values()):
				raise ValueError(f"invalid {counter_name} hit count for {source}")
		if any(not isinstance(value, (int, float)) or value < 0
				for values in record["b"].values() for value in values):
			raise ValueError(f"invalid branch hit count for {source}")
		reported_path = Path(record.get("path") or source).expanduser()
		if not reported_path.is_absolute():
			reported_path = Path.cwd() / reported_path
		canonical = str(reported_path.resolve())
		record["path"] = canonical
		if canonical in normalized:
			raise ValueError(f"coverage artifact contains duplicate source path: {canonical}")
		normalized[canonical] = record
	return normalized


def coverage_metrics(coverage):
	line_hits = {}
	function_hits = []
	branch_hits = []
	for record in coverage.values():
		for key, location in record["statementMap"].items():
			line = (record.get("path", ""), str(location.get("start", {}).get("line", key)))
			line_hits.setdefault(line, []).append(int(record["s"].get(key, 0)) > 0)
		function_hits.extend(int(record["f"].get(key, 0)) > 0 for key in record["fnMap"])
		for key in record["branchMap"]:
			branch_hits.extend(int(hit) > 0 for hit in record["b"].get(key, []))
	line_values = [any(hits) for hits in line_hits.values()]
	return {
		"lines": metric(line_values),
		"functions": metric(function_hits),
		"branches": metric(branch_hits),
	}


def metric(values):
	covered = sum(bool(value) for value in values)
	total = len(values)
	return {"covered": covered, "total": total, "pct": 100.0 if total == 0 else covered * 100 / total}


def print_coverage_metrics(label, metrics):
	print(f"Open Contract coverage (own sources): {label}")
	for name, value in metrics.items():
		print(f"  {name}: {value['covered']}/{value['total']} ({value['pct']:.2f}%)")


def main(argv=None):
	args = parse_args(sys.argv[1:] if argv is None else argv)
	try:
		contract_dir, selected_config = resolve_contract(args)
		contract = load_contract_tree(args, contract_dir, selected_config)
	except (OSError, ValueError, yaml.YAMLError) as error:
		return fail(str(error))
	with tempfile.TemporaryDirectory(prefix="open-contract-coverage-") as coverage_name:
		args.coverage_root = Path(coverage_name)
		args.root_directory = contract_dir
		try:
			status = execute_contract_tree(args, contract, contract_dir)
		except (OSError, ValueError, json.JSONDecodeError) as error:
			return fail(str(error))
		return status


if __name__ == "__main__":
	raise SystemExit(main())
