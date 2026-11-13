from __future__ import annotations

from pathlib import Path

from coverage import Coverage

from open_contracts_runner.cli import _coverage_report


def _collect_parallel_run(data_file: Path, source_file: Path, flag: bool) -> None:
    coverage = Coverage(
        data_file=str(data_file),
        source=[str(source_file.parent.parent)],
        branch=True,
        data_suffix=True,
        config_file=False,
    )
    coverage.start()
    exec(compile(source_file.read_text(encoding="utf-8"), str(source_file), "exec"), {
        "flag": flag,
    })
    adapter_file = source_file.parent.parent / "adapter.py"
    exec(compile(adapter_file.read_text(encoding="utf-8"), str(adapter_file), "exec"), {})
    coverage.stop()
    coverage.save()


def test_coverage_report_combines_parallel_dataset_runs(
    tmp_path: Path, capsys,
) -> None:
    source_dir = tmp_path / "plugin-src"
    source_dir.mkdir()
    source_file = source_dir / "measured.py"
    source_file.write_text(
        "if flag:\n    result = 'yes'\nelse:\n    result = 'no'\n",
        encoding="utf-8",
    )
    (tmp_path / "adapter.py").write_text("adapter_value = True\n", encoding="utf-8")
    data_file = tmp_path / ".coverage"
    _collect_parallel_run(data_file, source_file, True)
    _collect_parallel_run(data_file, source_file, False)

    assert len(list(tmp_path.glob(".coverage.*"))) == 2
    assert _coverage_report(data_file, [str(source_dir)], 100) == 0

    report = capsys.readouterr().err
    assert "100%" in report
    assert "measured.py" in report
    assert "adapter.py" not in report
    assert data_file.is_file()
    assert not list(tmp_path.glob(".coverage.*"))

    _collect_parallel_run(data_file, source_file, True)
    assert _coverage_report(data_file, [str(source_dir)], 100) == 0
    report = capsys.readouterr().err
    assert "100%" in report
    assert "adapter.py" not in report
    assert not list(tmp_path.glob(".coverage.*"))
