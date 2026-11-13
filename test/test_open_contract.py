import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace


MODULE_PATH = Path(__file__).resolve().parents[1] / "bin" / "open_contract.py"
SPEC = importlib.util.spec_from_file_location("open_contract", MODULE_PATH)
open_contract = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(open_contract)


class JavaScriptRunnerManifestTests(unittest.TestCase):
	def test_manifest_paths_are_contract_local_and_runner_paths_are_internal(self):
		with tempfile.TemporaryDirectory() as temporary:
			contract = Path(temporary)
			(contract / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
			(contract / "license-state.js").write_text("export {};\n", encoding="utf-8")
			(contract / "open-contract.yaml").write_text(
				"version: 1\ndataset: dataset.yaml\nrunner: javascript\n"
				"command: [run, dataset.yaml, --coverage-source, ./license-state.js, --coverage-threshold, '100']\n"
				"read: [./license-state.js]\n",
				encoding="utf-8",
			)
			args = SimpleNamespace(config=None, dataset=None, command=[])
			manifest = open_contract.load_manifest(args, contract, "open-contract.yaml")
			command, runner_reads = open_contract.prepare_command(manifest, [], contract)

			self.assertEqual(command[:2], ["node", str(open_contract.JAVASCRIPT_RUNNER_CLI)])
			self.assertIn("--coverage-command", command)
			self.assertIn(str(open_contract.JAVASCRIPT_COVERAGE_COMMAND), command)
			self.assertIn(open_contract.JAVASCRIPT_RUNNER, runner_reads)
			self.assertIn(
				open_contract.OPEN_CONTRACT_ROOT / "runner/bootstrap-contracts" / "predicates" / "predicate-language.schema.json",
				runner_reads,
			)
			self.assertEqual(manifest["read"], ["./license-state.js"])

	def test_runner_manifest_rejects_paths_outside_its_contract(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contract"
			contract.mkdir()
			(root / "shared.txt").write_text("outside\n", encoding="utf-8")
			(contract / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
			(contract / "open-contract.yaml").write_text(
				"version: 1\ndataset: dataset.yaml\nrunner: javascript\n"
				"command: [run, dataset.yaml]\nread: [../shared.txt]\n",
				encoding="utf-8",
			)
			args = SimpleNamespace(config=None, dataset=None, command=[])
			with self.assertRaisesRegex(ValueError, "must stay inside the contract directory"):
				open_contract.load_manifest(args, contract, "open-contract.yaml")

	def test_javascript_runner_command_cannot_be_supplied_by_contract(self):
		with tempfile.TemporaryDirectory() as temporary:
			contract = Path(temporary)
			(contract / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
			(contract / "coverage.js").write_text("export {};\n", encoding="utf-8")
			(contract / "open-contract.yaml").write_text(
				"version: 1\ndataset: dataset.yaml\nrunner: javascript\n"
				"command: [run, dataset.yaml, --coverage-command, ./c8]\n",
				encoding="utf-8",
			)
			args = SimpleNamespace(config=None, dataset=None, command=[])
			with self.assertRaisesRegex(ValueError, "Open Contract supplies the JavaScript coverage command"):
				open_contract.load_manifest(args, contract, "open-contract.yaml")

	def test_application_dependency_scan_does_not_walk_declared_child_contracts(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contracts/parent"
			child = contract / "children/child"
			(root / "package.json").write_text(
				open_contract.json.dumps({"imports": {"#contract/*": "./*"}, "dependencies": {}}),
				encoding="utf-8",
			)
			for path, contents in {
				contract / "open-contract-evaluator.mjs": "import value from './owned.mjs';\n",
				contract / "owned.mjs": "export default 1;\n",
				child / "child-evaluator.mjs": "import value from '#contract/child-source.js';\n",
				root / "child-source.js": "export default 1;\n",
				root / "src/parent-source.js": "export default 2;\n",
			}.items():
				path.parent.mkdir(parents=True, exist_ok=True)
				path.write_text(contents, encoding="utf-8")

			dependencies = open_contract.javascript_application_dependencies(contract, [child])

		self.assertIn((root / "package.json").resolve(), dependencies)
		self.assertIn((contract / "owned.mjs").resolve(), dependencies)
		self.assertNotIn((child / "child-evaluator.mjs").resolve(), dependencies)
		self.assertNotIn((root / "child-source.js").resolve(), dependencies)

	def test_package_can_declare_runner_dependencies_without_manifest_reads(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contracts/parent"
			dependency = root / "src/worker-test-config.js"
			contract.mkdir(parents=True)
			dependency.parent.mkdir()
			dependency.write_text("export default {};\n", encoding="utf-8")
			(root / "package.json").write_text(open_contract.json.dumps({
				"imports": {"#contract/*": "./*"},
				"openContractRunner": {"contracts": {
					"contracts/parent": {"runnerDependencies": ["src/worker-test-config.js"]},
				}},
			}), encoding="utf-8")

			dependencies = open_contract.javascript_application_dependencies(contract)

		self.assertIn(dependency.resolve(), dependencies)

	def test_application_coverage_sources_cannot_include_declared_child_contracts(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contracts/parent"
			child = contract / "children/child"
			source = child / "owned.js"
			source.parent.mkdir(parents=True)
			source.write_text("export {};\n", encoding="utf-8")
			(root / "package.json").write_text(open_contract.json.dumps({
				"imports": {"#contract/*": "./*"},
				"openContractRunner": {"contracts": {
					"contracts/parent": {"coverageSources": ["contracts/parent/children/child/owned.js"]},
				}},
			}), encoding="utf-8")

			with self.assertRaisesRegex(ValueError, "coverage source must not include a child contract"):
				open_contract.javascript_application_coverage_sources(contract, [child])

	def test_generic_runner_manifest_cannot_read_parent_resources(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contract"
			contract.mkdir()
			(root / "shared.txt").write_text("outside\n", encoding="utf-8")
			(contract / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
			(contract / "open-contract.yaml").write_text(
				"version: 1\ndataset: dataset.yaml\ncommand: [python3, -c, pass]\nread: [../shared.txt]\n",
				encoding="utf-8",
			)
			args = SimpleNamespace(config=None, dataset=None, command=[])
			with self.assertRaisesRegex(ValueError, "must stay inside the contract directory"):
				open_contract.load_manifest(args, contract, "open-contract.yaml")


class NestedContractManifestTests(unittest.TestCase):
	def write_manifest(self, contract, manifest):
		contract.mkdir(parents=True, exist_ok=True)
		(contract / "open-contract.yaml").write_text(
			open_contract.yaml.safe_dump(manifest, sort_keys=False), encoding="utf-8",
		)

	def write_dataset_contract(self, contract, label, *, children=None, exit_code=0, read=None):
		contract.mkdir(parents=True, exist_ok=True)
		(contract / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
		manifest = {
			"version": 1,
			"dataset": "dataset.yaml",
			"isolation": "none",
			"command": [sys.executable, "-c", f"print({label!r}); raise SystemExit({exit_code})"],
		}
		if children is not None:
			manifest["contracts"] = children
		if read is not None:
			manifest["read"] = read
		self.write_manifest(contract, manifest)

	def test_parent_dataset_runs_before_explicit_children_in_declared_order(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			self.write_dataset_contract(parent, "parent")
			manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/first", "children/second"]
			self.write_manifest(parent, manifest)
			self.write_manifest(parent / "children/first", {"version": 1, "contracts": ["nested/leaf"]})
			self.write_dataset_contract(parent / "children/first/nested/leaf", "leaf")
			self.write_dataset_contract(parent / "children/second", "second")
			self.write_dataset_contract(parent / "undeclared", "undeclared")

			result = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(parent)],
				capture_output=True, text=True, check=False)

		self.assertEqual(result.returncode, 0, result.stderr)
		self.assertLess(result.stdout.index("parent"), result.stdout.index("leaf"))
		self.assertLess(result.stdout.index("leaf"), result.stdout.index("second"))
		self.assertNotIn("undeclared", result.stdout)
		self.assertIn("Open Contract child result: children/first/nested/leaf (exit status 0)", result.stdout)

	def test_parent_may_own_dataset_and_explicitly_list_children(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			self.write_dataset_contract(parent, "parent")
			manifest_path = parent / "open-contract.yaml"
			manifest = open_contract.yaml.safe_load(manifest_path.read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/first", "children/second"]
			self.write_manifest(parent, manifest)
			self.write_dataset_contract(parent / "children/first", "first")
			self.write_dataset_contract(parent / "children/second", "second")

			args = SimpleNamespace(config=None, dataset=None, command=[])
			loaded = open_contract.load_manifest(args, parent, "open-contract.yaml")
			children = open_contract.child_contract_directories(loaded, parent)

		self.assertEqual([item.name for item in children], ["first", "second"])

	def test_parent_mount_file_list_excludes_declared_child_trees(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			child = parent / "children/child"
			self.write_dataset_contract(parent, "parent")
			self.write_dataset_contract(child, "child")
			(parent / "schema").mkdir()
			(parent / "schema/0001.sql").write_text("CREATE TABLE sample (id TEXT);\n", encoding="utf-8")
			(parent / "children/parent-owned.txt").write_text("owned by parent\n", encoding="utf-8")
			owned = open_contract.owned_contract_files(parent, [child])

		self.assertIn((parent / "dataset.yaml").resolve(), owned)
		self.assertIn((parent / "open-contract.yaml").resolve(), owned)
		self.assertIn((parent / "children/parent-owned.txt").resolve(), owned)
		self.assertIn((parent / "schema").resolve(), owned)
		self.assertNotIn((parent / "children").resolve(), owned)
		self.assertIn((parent / "schema/0001.sql").resolve(), owned)
		self.assertNotIn((child / "dataset.yaml").resolve(), owned)
		self.assertNotIn((child / "open-contract.yaml").resolve(), owned)

	def test_parent_cannot_declare_child_resources_as_own_reads(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			child = parent / "children/child"
			self.write_dataset_contract(parent, "parent")
			self.write_dataset_contract(child, "child")
			manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/child"]
			args = SimpleNamespace(config=None, dataset=None, command=[])
			for resource in ("children", "children/child/dataset.yaml"):
				manifest["read"] = [resource]
				self.write_manifest(parent, manifest)
				with self.subTest(resource=resource), self.assertRaisesRegex(ValueError, "must not include a child contract"):
					open_contract.load_manifest(args, parent, "open-contract.yaml")

	def test_parent_cannot_select_a_child_dataset_directly_or_by_override(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			child = parent / "children/child"
			self.write_dataset_contract(parent, "parent")
			self.write_dataset_contract(child, "child")
			manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/child"]
			manifest["dataset"] = "children/child/dataset.yaml"
			self.write_manifest(parent, manifest)
			args = SimpleNamespace(config=None, dataset=None, command=[])

			with self.assertRaisesRegex(ValueError, "manifest dataset must not be inside a child contract"):
				open_contract.load_manifest(args, parent, "open-contract.yaml")

			manifest["dataset"] = "dataset.yaml"
			self.write_manifest(parent, manifest)
			args.dataset = "children/child/dataset.yaml"
			with self.assertRaisesRegex(ValueError, "dataset override must not be inside a child contract"):
				open_contract.load_manifest(args, parent, "open-contract.yaml")

	def test_each_child_uses_its_own_runner_configuration(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			javascript = parent / "children/javascript"
			generic = parent / "children/generic"
			self.write_manifest(parent, {"version": 1, "contracts": ["children/javascript", "children/generic"]})
			self.write_dataset_contract(javascript, "javascript")
			javascript_manifest = open_contract.yaml.safe_load((javascript / "open-contract.yaml").read_text(encoding="utf-8"))
			javascript_manifest["runner"] = "javascript"
			javascript_manifest["command"] = ["run", "dataset.yaml"]
			self.write_manifest(javascript, javascript_manifest)
			self.write_dataset_contract(generic, "generic")
			args = SimpleNamespace(config=None, dataset=None, command=[])
			tree = open_contract.load_contract_tree(args, parent, "open-contract.yaml")
			js_command, js_reads = open_contract.prepare_command(tree["children"][0]["manifest"], [], javascript)
			generic_command, generic_reads = open_contract.prepare_command(tree["children"][1]["manifest"], [], generic)

		self.assertEqual(js_command[:2], ["node", str(open_contract.JAVASCRIPT_RUNNER_CLI)])
		self.assertIn(open_contract.JAVASCRIPT_RUNNER, js_reads)
		self.assertEqual(generic_command[0], sys.executable)
		self.assertEqual(generic_reads, [])

	def test_single_dataset_manifest_remains_executable(self):
		with tempfile.TemporaryDirectory() as temporary:
			contract = Path(temporary) / "single"
			self.write_dataset_contract(contract, "legacy-single")

			result = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(contract)],
				capture_output=True, text=True, check=False)

		self.assertEqual(result.returncode, 0, result.stderr)
		self.assertIn("legacy-single", result.stdout)

	def test_dataset_override_remains_supported_for_single_dataset_contract(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			self.write_dataset_contract(parent, "parent")
			(parent / "override.yaml").write_text("version: 1\n", encoding="utf-8")
			parent_manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			parent_manifest["command"] = [sys.executable, "-c", "import os; print(os.environ['OPEN_CONTRACT_DATASET'])"]
			self.write_manifest(parent, parent_manifest)

			result = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(parent),
				"--dataset", "override.yaml"], capture_output=True, text=True, check=False)

		self.assertEqual(result.returncode, 0, result.stderr)
		self.assertIn(str(parent / "override.yaml"), result.stdout)

	def test_parent_propagates_child_failure_and_continues_ordered_children(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			self.write_manifest(parent, {"version": 1, "contracts": ["children/failing", "children/later"]})
			self.write_dataset_contract(parent / "children/failing", "failing", exit_code=7)
			self.write_dataset_contract(parent / "children/later", "later")

			result = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(parent)],
				capture_output=True, text=True, check=False)

		self.assertEqual(result.returncode, 7, result.stderr)
		self.assertIn("later", result.stdout)
		self.assertIn("children/failing (exit status 7)", result.stdout)

	def test_child_resource_paths_are_resolved_from_child_directory_only(self):
		with tempfile.TemporaryDirectory() as temporary:
			parent = Path(temporary) / "parent"
			child = parent / "children/child"
			(parent / "parent.txt").parent.mkdir(parents=True, exist_ok=True)
			(parent / "parent.txt").write_text("parent\n", encoding="utf-8")
			(child / "child.txt").parent.mkdir(parents=True, exist_ok=True)
			(child / "child.txt").write_text("child\n", encoding="utf-8")
			(parent / "sibling").mkdir()
			(parent / "sibling/sibling.txt").write_text("sibling\n", encoding="utf-8")
			self.write_manifest(parent, {"version": 1, "contracts": ["children/child"]})
			self.write_dataset_contract(child, "child", read=["child.txt"])
			args = SimpleNamespace(config=None, dataset=None, command=[])
			tree = open_contract.load_contract_tree(args, parent, "open-contract.yaml")
			child_manifest = tree["children"][0]["manifest"]
			child_args = SimpleNamespace(
				read=child_manifest["read"], write=[], manifest_read=child_manifest["read"],
				manifest_write=[], enforce_manifest_ownership=True,
			)
			reads, _ = open_contract.collect_paths(child_args, child, tree["children"][0]["dataset"])

		self.assertIn(child.resolve(), reads)
		self.assertIn((child / "child.txt").resolve(), reads)
		self.assertNotIn(parent.resolve(), reads)
		self.assertNotIn((parent / "parent.txt").resolve(), reads)
		self.assertNotIn((parent / "sibling").resolve(), reads)

	def test_parent_rejects_duplicate_self_and_escaping_child_directories(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			parent = root / "parent"
			child = parent / "child"
			(root / "outside").mkdir()
			self.write_dataset_contract(child, "child")
			for paths, message in ((["child", "./child"], "duplicate child"),
								(["."], "stay inside"), (["../outside"], "stay inside")):
				self.write_manifest(parent, {"version": 1, "contracts": paths})
				args = SimpleNamespace(config=None, dataset=None, command=[])
				with self.subTest(paths=paths), self.assertRaisesRegex((ValueError, OSError), message):
					open_contract.load_manifest(args, parent, "open-contract.yaml")

	def test_manifest_dataset_path_must_be_relative_and_inside_contract(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			contract = root / "contract"
			contract.mkdir()
			(root / "outside.yaml").write_text("version: 1\n", encoding="utf-8")
			args = SimpleNamespace(config=None, dataset=None, command=[])
			for dataset, message in ((str(root / "outside.yaml"), "must be relative"),
									("../outside.yaml", "stay inside")):
				self.write_manifest(contract, {
					"version": 1, "dataset": dataset, "command": ["python3", "-c", "pass"],
				})
				with self.subTest(dataset=dataset), self.assertRaisesRegex(ValueError, message):
					open_contract.load_manifest(args, contract, "open-contract.yaml")


class CoverageOwnershipTests(unittest.TestCase):
	def coverage(self, source, *, count, line=1):
		return {
			source: {
				"path": source,
				"statementMap": {"0": {"start": {"line": line}, "end": {"line": line}}},
				"fnMap": {"0": {"name": "work", "loc": {"start": {"line": line}, "end": {"line": line}}}},
				"branchMap": {"0": {"type": "if", "locations": [{"start": {"line": line}, "end": {"line": line}}]}},
				"s": {"0": count}, "f": {"0": count}, "b": {"0": [count]},
			},
		}

	def test_line_is_covered_when_any_statement_on_that_line_was_hit(self):
		source = "/tmp/source.js"
		coverage = self.coverage(source, count=0)
		record = coverage[source]
		record["statementMap"]["1"] = {"start": {"line": 1}, "end": {"line": 1}}
		record["s"]["1"] = 1

		self.assertEqual(open_contract.coverage_metrics(coverage)["lines"], {
			"covered": 1, "total": 1, "pct": 100.0,
		})

	def test_parent_and_child_coverage_reports_and_gates_are_independent(self):
		with tempfile.TemporaryDirectory() as temporary:
			root = Path(temporary)
			parent = root / "parent"
			child = parent / "children/child"
			source = str(root / "source.js")

			def configure(directory, count):
				coverage = self.coverage(source, count=count)
				command = [sys.executable, "-c", (
					"import json, os; json.dump(" + repr(coverage)
					+ ", open(os.environ['OPEN_CONTRACT_COVERAGE_ARTIFACT'], 'w'))"
				)]
				directory.mkdir(parents=True, exist_ok=True)
				(directory / "dataset.yaml").write_text("version: 1\n", encoding="utf-8")
				(directory / "open-contract.yaml").write_text(open_contract.yaml.safe_dump({
					"version": 1, "dataset": "dataset.yaml", "isolation": "none", "command": command,
				}, sort_keys=False), encoding="utf-8")

			configure(parent, 0)
			configure(child, 1)
			manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/child"]
			(parent / "open-contract.yaml").write_text(open_contract.yaml.safe_dump(manifest, sort_keys=False), encoding="utf-8")

			parent_fails = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(parent)],
				capture_output=True, text=True, check=False)
			self.assertEqual(parent_fails.returncode, 1, parent_fails.stderr)
			self.assertIn("Open Contract coverage (own sources): .", parent_fails.stdout)
			self.assertIn("Open Contract coverage (own sources): children/child", parent_fails.stdout)
			self.assert_coverage(parent_fails.stdout, ".", 0)
			self.assert_coverage(parent_fails.stdout, "children/child", 1)
			self.assertNotIn("parent suite aggregate", parent_fails.stdout)

			configure(parent, 1)
			configure(child, 0)
			manifest = open_contract.yaml.safe_load((parent / "open-contract.yaml").read_text(encoding="utf-8"))
			manifest["contracts"] = ["children/child"]
			(parent / "open-contract.yaml").write_text(open_contract.yaml.safe_dump(manifest, sort_keys=False), encoding="utf-8")
			child_fails = subprocess.run([sys.executable, str(MODULE_PATH), "run", str(parent)],
				capture_output=True, text=True, check=False)
			self.assertEqual(child_fails.returncode, 1, child_fails.stderr)
			self.assert_coverage(child_fails.stdout, ".", 1)
			self.assert_coverage(child_fails.stdout, "children/child", 0)
			self.assertNotIn("parent suite aggregate", child_fails.stdout)

	def assert_coverage(self, output, owner, covered):
		percentage = "100.00%" if covered else "0.00%"
		header = f"Open Contract coverage (own sources): {owner}\n"
		self.assertIn(header, output)
		block = output.split(header, 1)[1].split("\nOpen Contract", 1)[0]
		for metric_name in ("lines", "functions", "branches"):
			self.assertIn(f"  {metric_name}: {covered}/1 ({percentage})", block)

if __name__ == "__main__":
	unittest.main()
