import importlib.util
import tempfile
import unittest
from pathlib import Path


CHECKER = Path(__file__).with_name("check_bootstrap_contract_coverage.py")
SPEC = importlib.util.spec_from_file_location("bootstrap_coverage", CHECKER)
bootstrap_coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap_coverage)


class LcovLineParsingTests(unittest.TestCase):
    def test_ignores_brace_only_lines_but_keeps_uncovered_statements(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.rs"
            source.write_text("fn run() {\n    work();\n}\n", encoding="utf-8")
            contents = "\n".join(
                [
                    f"SF:{source}",
                    "DA:1,1",
                    "DA:2,0",
                    "DA:3,0",
                    "end_of_record",
                ]
            )

            result = bootstrap_coverage.parse_lcov_lines(
                contents, [{"filename": str(source)}]
            )

            executable, covered = result[source.resolve()]
            self.assertEqual(executable, {1, 2})
            self.assertEqual(covered, {1})


if __name__ == "__main__":
    unittest.main()
