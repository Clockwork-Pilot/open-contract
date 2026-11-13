use open_contract_core::{run_contract_tree, Backend, BatchReport, ManifestBatchAdapter};
use open_contract_errors::ContractError;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("open-contract-core: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn run() -> Result<(), ContractError> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("run-manifest") {
        return Err((2, "usage: open-contract-core run-manifest <contract.yaml> --isolation <none|bwrap|landlock> [--coverage-report <path>]".into()).into());
    }
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    let mut backend = None;
    let mut coverage_report = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_backend(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?)
            }
            "--coverage-report" => {
                if coverage_report.is_some() {
                    return Err((2, "--coverage-report may only be specified once".into()).into());
                }
                coverage_report = Some(PathBuf::from(
                    args.next()
                        .ok_or((2, "missing coverage report path".into()))?,
                ));
            }
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    let backend = backend.ok_or((2, "--isolation is required".into()))?;
    let report = run_contract_tree(manifest, backend, &ManifestBatchAdapter)
        .map_err(|error| ContractError::from((2, error)).context("contract execution"))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| (2, e.to_string()))?
    );
    if let Some(path) = coverage_report {
        write_coverage_report(&path, &report)?;
        eprintln!("coverage report written to {}", path.display());
    }
    if report.passed() != report.total() {
        let mismatches = coverage_fingerprint_mismatches(&report);
        let mismatch_summary = if mismatches.is_empty() {
            String::new()
        } else {
            format!("; {mismatches}")
        };
        return Err((
            1,
            format!(
                "{} of {} contract cases passed{}",
                report.passed(),
                report.total(),
                mismatch_summary
            ),
        )
            .into());
    }
    let supplemental_failures = report
        .coverage
        .iter()
        .filter(|record| record.additional_test_coverage["passed"] == false)
        .map(|record| {
            let detail = record.additional_test_coverage["error"]
                .as_str()
                .unwrap_or("no adapter error detail");
            format!("{}: {detail}", record.contract)
        })
        .collect::<Vec<_>>();
    if !supplemental_failures.is_empty() {
        return Err((
            1,
            format!(
                "supplemental contract tests failed: {}",
                supplemental_failures.join("; ")
            ),
        )
            .into());
    }
    if !report.coverage_complete() {
        let missing = report
            .coverage
            .iter()
            .filter(|record| !record.missing_sources.is_empty())
            .map(|record| format!("{}: {}", record.contract, record.missing_sources.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        return Err((
            1,
            format!("coverage did not exercise every declared contract source: {missing}"),
        )
            .into());
    }
    Ok(())
}

fn coverage_fingerprint_mismatches(report: &BatchReport) -> String {
    let mut mismatches = Vec::new();
    for result in &report.results {
        if let Some(coverage) = &result.coverage {
            if coverage.baseline_matches == Some(false) {
                mismatches.push(format!(
                    "{}/{}/{} expected {}, observed {}",
                    result.contract,
                    result.target_id,
                    result.case_id,
                    coverage
                        .expected_fingerprint
                        .as_deref()
                        .unwrap_or("<missing>"),
                    coverage.fingerprint
                ));
            }
        }
        for step in &result.steps {
            if let Some(coverage) = &step.coverage {
                if coverage.baseline_matches == Some(false) {
                    mismatches.push(format!(
                        "{}/{}/{}/{} expected {}, observed {}",
                        result.contract,
                        result.target_id,
                        result.case_id,
                        step.id,
                        coverage
                            .expected_fingerprint
                            .as_deref()
                            .unwrap_or("<missing>"),
                        coverage.fingerprint
                    ));
                }
            }
        }
    }
    if mismatches.is_empty() {
        String::new()
    } else {
        format!("coverage fingerprint mismatches: {}", mismatches.join("; "))
    }
}

fn write_coverage_report(
    path: &std::path::Path,
    report: &open_contract_core::BatchReport,
) -> Result<(), ContractError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            (
                2,
                format!(
                    "cannot create coverage report directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let contracts = report
        .coverage
        .iter()
        .map(|record| {
            let contract_results = report
                .results
                .iter()
                .filter(|result| result.contract == record.contract)
                .collect::<Vec<_>>();
            let mut lines_by_file = BTreeMap::<String, BTreeSet<u32>>::new();
            for result in &contract_results {
                if let Some(coverage) = &result.coverage {
                    add_coverage_files(&mut lines_by_file, &coverage.files);
                }
                for step in &result.steps {
                    if let Some(coverage) = &step.coverage {
                        add_coverage_files(&mut lines_by_file, &coverage.files);
                    }
                }
            }
            let mut normalized = record
                .line_coverage
                .iter()
                .map(|file| (file.path.clone(), file))
                .collect::<BTreeMap<_, _>>();
            let mut source_paths = record.declared_sources.clone();
            source_paths.sort();
            source_paths.dedup();
            let files = source_paths
                .into_iter()
                .map(|source| {
                    let file = normalized.remove(&source);
                    let executable = file
                        .map(|file| file.executable_lines.clone())
                        .unwrap_or_default();
                    let mut covered = file
                        .map(|file| file.covered_lines.clone())
                        .unwrap_or_default();
                    if covered.is_empty() {
                        covered.extend(lines_by_file.remove(&source).unwrap_or_default());
                    }
                    let covered_set = covered.iter().copied().collect::<BTreeSet<_>>();
                    let uncovered = executable
                        .iter()
                        .filter(|line| !covered_set.contains(line))
                        .copied()
                        .collect::<Vec<_>>();
                    let executable_line_count = executable.len();
                    let covered_line_count = covered.len();
                    let uncovered_line_count = uncovered.len();
                    json!({
                        "path": source,
                        "executable_lines": executable,
                        "executable_line_count": executable_line_count,
                        "covered_lines": covered,
                        "covered_line_count": covered_line_count,
                        "uncovered_lines": uncovered,
                        "uncovered_line_count": uncovered_line_count,
                    })
                })
                .collect::<Vec<_>>();
            let additional_files = line_file_report(&record.declared_sources, &record.additional_line_coverage);
            let mut additional_test_coverage = record.additional_test_coverage.clone();
            if let Some(details) = additional_test_coverage.as_object_mut() {
                details.insert("files".into(), json!(additional_files));
            }
            let combined_files = combined_line_file_report(
                &record.declared_sources,
                &record.line_coverage,
                &record.additional_line_coverage,
            );
            let cases = contract_results
                .into_iter()
                .map(|result| {
                    json!({
                        "target_id": result.target_id,
                        "case_id": result.case_id,
                        "coverage": result.coverage,
                        "steps": result.steps.iter().map(|step| json!({
                            "step_id": step.id,
                            "coverage": step.coverage,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "contract": record.contract,
                "native_report": record.native_report,
                "declared_sources": record.declared_sources,
                "covered_sources": record.covered_sources,
                "missing_sources": record.missing_sources,
                "dataset_coverage": {
                    "files": files,
                    "note": "Uncovered lines are executable source lines not reached by the dataset batch. Empty line lists mean the adapter did not provide normalized executable-line data for that source.",
                    "cases": cases,
                },
                "additional_test_coverage": additional_test_coverage,
                "combined_coverage": {"files": combined_files},
            })
        })
        .collect::<Vec<_>>();
    let coverage = json!({
        "coverage_complete": report.coverage_complete(),
        "contracts": contracts,
    });
    let contents = serde_json::to_vec_pretty(&coverage)
        .map_err(|error| (2, format!("cannot serialize coverage report: {error}")))?;
    fs::write(path, contents)
        .map_err(|error| {
            (
                2,
                format!("cannot write coverage report {}: {error}", path.display()),
            )
        })
        .map_err(ContractError::from)?;
    eprint!("{}", coverage_line_summary(&coverage));
    Ok(())
}

fn coverage_line_summary(report: &serde_json::Value) -> String {
    let mut output = String::from("Dataset line coverage:\n");
    for contract in report["contracts"].as_array().into_iter().flatten() {
        let name = contract["contract"]
            .as_str()
            .map_or("<unnamed>", |value| value);
        output.push_str(&format!("  {name}:\n"));
        for file in contract["dataset_coverage"]["files"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = file["path"].as_str().map_or("<unknown>", |value| value);
            let executable = file["executable_line_count"]
                .as_u64()
                .map_or(0, |value| value);
            let covered = file["covered_line_count"].as_u64().map_or(0, |value| value);
            let uncovered = file["uncovered_lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_u64)
                .filter_map(|line| u32::try_from(line).ok())
                .collect::<Vec<_>>();
            if executable == 0 {
                output.push_str(&format!("    {path}: normalized line data unavailable\n"));
            } else if uncovered.is_empty() {
                output.push_str(&format!(
                    "    {path}: all {covered}/{executable} executable lines covered\n"
                ));
            } else {
                output.push_str(&format!(
                    "    {path}: {covered}/{executable} covered; uncovered lines {}\n",
                    format_line_ranges(&uncovered)
                ));
            }
        }
    }
    for contract in report["contracts"].as_array().into_iter().flatten() {
        let status = contract["additional_test_coverage"]["status"]
            .as_str()
            .unwrap_or("not-declared");
        if status != "not-declared" {
            output.push_str(&format!(
                "  {} supplemental tests: {status}\n",
                contract["contract"].as_str().unwrap_or("<unnamed>")
            ));
            for file in contract["additional_test_coverage"]["files"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let path = file["path"].as_str().unwrap_or("<unknown>");
                let covered = file["covered_lines"].as_array().map_or(0, Vec::len);
                let executable = file["executable_lines"].as_array().map_or(0, Vec::len);
                let uncovered = file["uncovered_lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_u64)
                    .filter_map(|line| u32::try_from(line).ok())
                    .collect::<Vec<_>>();
                if executable > 0 {
                    output.push_str(&format!(
                        "    {path}: {covered}/{executable} covered; uncovered lines {}\n",
                        format_line_ranges(&uncovered)
                    ));
                }
            }
        }
    }
    output
}

fn line_file_report(
    sources: &[String],
    coverage: &[open_contract_core::batch::CoverageLineFile],
) -> Vec<serde_json::Value> {
    sources
        .iter()
        .map(|source| {
            let file = coverage.iter().find(|file| &file.path == source);
            let executable = file.map_or_else(Vec::new, |file| file.executable_lines.clone());
            let covered = file.map_or_else(Vec::new, |file| file.covered_lines.clone());
            let executable_set = executable.iter().copied().collect::<BTreeSet<_>>();
            let covered_set = covered
                .iter()
                .copied()
                .filter(|line| executable_set.contains(line))
                .collect::<BTreeSet<_>>();
            let uncovered = executable_set
                .difference(&covered_set)
                .copied()
                .collect::<Vec<_>>();
            json!({
                "path": source,
                "executable_lines": executable_set,
                "covered_lines": covered_set,
                "uncovered_lines": uncovered,
            })
        })
        .collect()
}

fn combined_line_file_report(
    sources: &[String],
    dataset: &[open_contract_core::batch::CoverageLineFile],
    additional: &[open_contract_core::batch::CoverageLineFile],
) -> Vec<serde_json::Value> {
    sources
        .iter()
        .map(|source| {
            let dataset_file = dataset.iter().find(|file| &file.path == source);
            let additional_file = additional.iter().find(|file| &file.path == source);
            let executable = dataset_file
                .map(|file| {
                    file.executable_lines
                        .iter()
                        .copied()
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            let mut covered = dataset_file
                .map(|file| file.covered_lines.iter().copied().collect::<BTreeSet<_>>())
                .unwrap_or_default();
            if let Some(file) = additional_file {
                covered.extend(file.covered_lines.iter().copied());
            }
            covered.retain(|line| executable.contains(line));
            let uncovered = executable.difference(&covered).copied().collect::<Vec<_>>();
            json!({
                "path": source,
                "executable_line_count": executable.len(),
                "covered_line_count": covered.len(),
                "uncovered_lines": uncovered,
            })
        })
        .collect()
}

fn format_line_ranges(lines: &[u32]) -> String {
    let mut sorted = lines.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let Some(&first) = sorted.first() else {
        return String::new();
    };
    let mut ranges = Vec::new();
    let (mut start, mut previous) = (first, first);
    for line in sorted.into_iter().skip(1) {
        if previous.checked_add(1) == Some(line) {
            previous = line;
            continue;
        }
        ranges.push(if start == previous {
            start.to_string()
        } else {
            format!("{start}-{previous}")
        });
        start = line;
        previous = line;
    }
    ranges.push(if start == previous {
        start.to_string()
    } else {
        format!("{start}-{previous}")
    });
    ranges.join(", ")
}

fn add_coverage_files(
    by_file: &mut BTreeMap<String, BTreeSet<u32>>,
    files: &[open_contract_core::batch::CoverageFile],
) {
    for file in files {
        by_file
            .entry(file.path.clone())
            .or_default()
            .extend(file.lines.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::{coverage_fingerprint_mismatches, coverage_line_summary};
    use open_contract_core::batch::{CoverageFingerprint, StepResult};
    use open_contract_core::{BatchReport, ContractResult};

    #[test]
    fn fingerprint_mismatch_summary_names_scope_and_both_values() {
        let coverage = CoverageFingerprint {
            files: Vec::new(),
            fingerprint: "observed-hash".into(),
            expected_fingerprint: Some("expected-hash".into()),
            baseline_matches: Some(false),
        };
        let report = BatchReport {
            results: vec![ContractResult {
                contract: "contract".into(),
                target_id: "target".into(),
                case_id: "case".into(),
                output: None,
                passed: false,
                matched_by: None,
                error: Some("coverage fingerprint mismatch".into()),
                steps: vec![StepResult {
                    id: "step".into(),
                    output: None,
                    passed: false,
                    matched_by: None,
                    error: Some("coverage fingerprint mismatch".into()),
                    coverage: Some(coverage.clone()),
                }],
                coverage: Some(coverage),
            }],
            coverage: Vec::new(),
        };

        let summary = coverage_fingerprint_mismatches(&report);
        assert!(
            summary.contains("contract/target/case expected expected-hash, observed observed-hash")
        );
        assert!(summary
            .contains("contract/target/case/step expected expected-hash, observed observed-hash"));
    }

    #[test]
    fn line_summary_names_uncovered_lines_as_compact_ranges() {
        let report = serde_json::json!({
            "contracts": [{
                "contract": "example",
                "dataset_coverage": {"files": [{
                    "path": "validator.py",
                    "executable_line_count": 8,
                    "covered_line_count": 5,
                    "uncovered_lines": [4, 5, 6]
                }]}
            }]
        });

        let summary = coverage_line_summary(&report);
        assert!(summary.contains("validator.py: 5/8 covered; uncovered lines 4-6"));
    }
}

fn parse_backend(value: &str) -> Result<Backend, ContractError> {
    match value {
        "none" => Ok(Backend::None),
        "bwrap" => Ok(Backend::Bubblewrap),
        "landlock" => Ok(Backend::Landlock),
        _ => Err((
            2,
            format!("invalid isolation backend {value:?}; expected none, bwrap, or landlock"),
        )
            .into()),
    }
}
