import importlib.util
import tempfile
import unittest
from pathlib import Path


CHECKER = Path(__file__).with_name("check_bootstrap_contract_coverage.py")
SPEC = importlib.util.spec_from_file_location("bootstrap_coverage", CHECKER)
bootstrap_coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap_coverage)


class DatasetManifestTests(unittest.TestCase):
    def write_manifest(self, root, contracts):
        import json

        (root / "datasets.manifest.json").write_text(
            json.dumps({"version": 1, "contracts": contracts}), encoding="utf-8"
        )

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.package = self.root / "example"
        self.package.mkdir()
        (self.package / "first.json").write_text(
            '{"version":1,"cases":[{"id":"first"}]}', encoding="utf-8"
        )
        (self.package / "second.yaml").write_text(
            "version: 1\ncases:\n  - id: second\n", encoding="utf-8"
        )
        (self.package / "schema.json").write_text("{}", encoding="utf-8")

    def tearDown(self):
        self.temporary.cleanup()

    def test_loads_multiple_datasets_in_declared_order_with_schema_refs(self):
        self.write_manifest(
            self.root,
            [{
                "id": "example",
                "package": "example",
                "datasets": [
                    {"id": "first", "path": "first.json", "schema": None},
                    {"id": "second", "path": "second.yaml", "schema": "schema.json"},
                ],
            }],
        )

        contract_id, datasets = bootstrap_coverage.load_dataset_declarations(self.package)

        self.assertEqual(contract_id, "example")
        self.assertEqual([item["id"] for item in datasets], ["first", "second"])
        self.assertIsNone(datasets[0]["schema"])
        self.assertEqual(datasets[1]["schema"], "schema.json")

    def test_rejects_missing_manifest_and_duplicate_dataset_declarations(self):
        with self.assertRaisesRegex(ValueError, "missing declaration manifest"):
            bootstrap_coverage.load_dataset_declarations(self.package)

        self.write_manifest(
            self.root,
            [{
                "id": "example",
                "package": "example",
                "datasets": [
                    {"id": "same", "path": "first.json", "schema": None},
                    {"id": "same", "path": "second.yaml", "schema": None},
                ],
            }],
        )
        with self.assertRaisesRegex(ValueError, "duplicate dataset ID"):
            bootstrap_coverage.load_dataset_declarations(self.package)

    def test_rejects_duplicate_contract_and_package_declarations(self):
        declaration = {
            "id": "example",
            "package": "example",
            "datasets": [{"id": "first", "path": "first.json", "schema": None}],
        }
        self.write_manifest(self.root, [declaration, declaration])
        with self.assertRaisesRegex(ValueError, "duplicate contract ID"):
            bootstrap_coverage.load_dataset_declarations(self.package)

    def test_rejects_dataset_paths_that_escape_package(self):
        self.write_manifest(
            self.root,
            [{
                "id": "example",
                "package": "example",
                "datasets": [{"id": "escape", "path": "../outside.json", "schema": None}],
            }],
        )
        with self.assertRaisesRegex(ValueError, "escapes its declared root"):
            bootstrap_coverage.load_dataset_declarations(self.package)

    def test_rejects_unsupported_dataset_and_schema_formats(self):
        self.write_manifest(
            self.root,
            [{
                "id": "example",
                "package": "example",
                "datasets": [{"id": "invalid", "path": "first.txt", "schema": None}],
            }],
        )
        (self.package / "first.txt").write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "unsupported dataset format"):
            bootstrap_coverage.load_dataset_declarations(self.package)

        self.write_manifest(
            self.root,
            [{
                "id": "example",
                "package": "example",
                "datasets": [{"id": "first", "path": "first.json", "schema": "schema.yaml"}],
            }],
        )
        (self.package / "schema.yaml").write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "schema must be a JSON file"):
            bootstrap_coverage.load_dataset_declarations(self.package)


if __name__ == "__main__":
    unittest.main()
