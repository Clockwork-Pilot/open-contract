use open_contract_core::batch::{ContractResult, CoverageLineFile, CoverageRecord};
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
    let mut coverage_report = PathBuf::from(".open-contract/coverage/latest.json");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_backend(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?)
            }
            "--coverage-report" => {
                coverage_report = PathBuf::from(
                    args.next()
                        .ok_or((2, "missing coverage report path".into()))?,
                );
            }
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    let backend = backend.ok_or((2, "--isolation is required".into()))?;
    let report = run_contract_tree(manifest, backend, &ManifestBatchAdapter)
        .map_err(|error| ContractError::from((2, error)).context("contract execution"))?;
    write_coverage_report(&coverage_report, &report)?;
    print!("{}", concise_summary(&report));
    println!("Coverage report: {}", coverage_report.display());
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
                "{}/{} dataset cases passed across {} contract(s){}",
                report.passed(),
                report.total(),
                report.coverage.len(),
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
    if !coverage_requirements_met(&report) {
        let failures = coverage_requirement_failures(&report).join("; ");
        return Err((1, format!("coverage requirements not met: {failures}")).into());
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
                    add_coverage_files(
                        &mut lines_by_file,
                        &coverage.files,
                        &record.declared_sources,
                    );
                }
                for step in &result.steps {
                    if let Some(coverage) = &step.coverage {
                        add_coverage_files(
                            &mut lines_by_file,
                            &coverage.files,
                            &record.declared_sources,
                        );
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
                    let executable_set = executable.iter().copied().collect::<BTreeSet<_>>();
                    let covered = lines_by_file
                        .get(&source)
                        .into_iter()
                        .flatten()
                        .filter(|line| executable_set.contains(line))
                        .copied()
                        .collect::<Vec<_>>();
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
            let scoped_dataset_coverage = record
                .line_coverage
                .iter()
                .map(|file| CoverageLineFile {
                    path: file.path.clone(),
                    executable_lines: file.executable_lines.clone(),
                    covered_lines: lines_by_file
                        .get(&file.path)
                        .map(|lines| lines.iter().copied().collect())
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>();
            let combined_files = combined_line_file_report(
                &record.declared_sources,
                &scoped_dataset_coverage,
                &record.additional_line_coverage,
            );
            let target_coverage = target_coverage_report(&contract_results, record);
            let target_coverage_complete = target_coverage
                .iter()
                .all(|target| target["coverage_complete"] == true)
                && !target_coverage.is_empty();
            let target_threshold_met = target_coverage
                .iter()
                .all(|target| target["threshold_met"] == true)
                && !target_coverage.is_empty();
            let scoped_covered_lines = files
                .iter()
                .map(|file| file["covered_line_count"].as_u64().unwrap_or(0) as usize)
                .sum::<usize>();
            let scoped_executable_lines = files
                .iter()
                .map(|file| file["executable_line_count"].as_u64().unwrap_or(0) as usize)
                .sum::<usize>();
            let reportable_coverage = target_coverage_complete
                && record.missing_sources.is_empty()
                && scoped_executable_lines > 0;
            let combined_covered_lines = combined_files
                .iter()
                .map(|file| file["covered_line_count"].as_u64().unwrap_or(0) as usize)
                .sum::<usize>();
            let combined_executable_lines = combined_files
                .iter()
                .map(|file| file["executable_line_count"].as_u64().unwrap_or(0) as usize)
                .sum::<usize>();
            let cases = contract_results
                .into_iter()
                .map(|result| {
                    json!({
                        "target_id": result.target_id,
                        "case_id": result.case_id,
                        "passed": result.passed,
                        "output": result.output,
                        "error": result.error,
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
                "minimum_line_percent": record.minimum_line_percent,
                "dataset_coverage_percent": reportable_coverage.then(|| percent(scoped_covered_lines, scoped_executable_lines)),
                "dataset_covered_lines": reportable_coverage.then_some(scoped_covered_lines),
                "dataset_executable_lines": reportable_coverage.then_some(scoped_executable_lines),
                "combined_coverage_percent": (reportable_coverage && record.additional_test_coverage["passed"] != false).then(|| percent(combined_covered_lines, combined_executable_lines)),
                "combined_covered_lines": (reportable_coverage && record.additional_test_coverage["passed"] != false).then_some(combined_covered_lines),
                "combined_executable_lines": (reportable_coverage && record.additional_test_coverage["passed"] != false).then_some(combined_executable_lines),
                "line_threshold_met": target_threshold_met && record.line_threshold_met,
                "adapter_line_threshold_met": record.line_threshold_met,
                "target_coverage_complete": target_coverage_complete,
                "target_coverage": target_coverage,
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
        "coverage_complete": coverage_requirements_met(report),
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
    Ok(())
}

fn coverage_line_summary(report: &serde_json::Value) -> String {
    let mut output = String::new();
    for contract in report["contracts"].as_array().into_iter().flatten() {
        let name = contract["contract"]
            .as_str()
            .map_or("<unnamed>", |value| value);
        let threshold = contract["minimum_line_percent"].as_f64().unwrap_or(100.0);
        let targets = contract["target_coverage"].as_array().into_iter().flatten();
        for target in targets {
            let target_id = target["target_id"].as_str().unwrap_or("<unnamed>");
            let cases = target["case_count"].as_u64().unwrap_or(0);
            let passed = target["passed_cases"].as_u64().unwrap_or(0);
            if target["coverage_complete"] == true {
                let percent = target["coverage_percent"].as_f64().unwrap_or(0.0);
                let covered = target["covered_lines"].as_u64().unwrap_or(0);
                let executable = target["executable_lines"].as_u64().unwrap_or(0);
                output.push_str(&format!("  {name} target {target_id}: {passed}/{cases} cases passed; {covered}/{executable} target lines covered ({percent:.1}%, required {threshold:.1}%)\n"));
            } else {
                let reason = target["incomplete_reason"]
                    .as_str()
                    .unwrap_or("coverage data is incomplete");
                output.push_str(&format!("  {name} target {target_id}: {passed}/{cases} cases passed; target coverage unavailable ({reason})\n"));
            }
            for file in target["files"].as_array().into_iter().flatten() {
                let path = file["path"].as_str().unwrap_or("<unknown>");
                let executable = file["executable_line_count"].as_u64().unwrap_or(0);
                let covered = file["covered_line_count"].as_u64().unwrap_or(0);
                let uncovered = file["uncovered_lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_u64)
                    .filter_map(|line| u32::try_from(line).ok())
                    .collect::<Vec<_>>();
                if executable > 0 && !uncovered.is_empty() {
                    output.push_str(&format!(
                        "    {path}: {covered}/{executable} target lines; uncovered lines {}\n",
                        format_line_ranges(&uncovered)
                    ));
                }
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

fn target_coverage_report(
    results: &[&ContractResult],
    record: &CoverageRecord,
) -> Vec<serde_json::Value> {
    let mut grouped = BTreeMap::<String, Vec<&ContractResult>>::new();
    for result in results {
        grouped
            .entry(result.target_id.clone())
            .or_default()
            .push(result);
    }
    grouped
        .into_iter()
        .map(|(target_id, cases)| {
            let mut lines_by_file = BTreeMap::<String, BTreeSet<u32>>::new();
            let mut scopes_complete = !cases.is_empty();
            for result in &cases {
                if let Some(coverage) = &result.coverage {
                    if coverage.files.is_empty() || coverage.baseline_matches == Some(false) {
                        scopes_complete = false;
                    }
                    scopes_complete &= add_coverage_files(
                        &mut lines_by_file,
                        &coverage.files,
                        &record.declared_sources,
                    );
                } else {
                    scopes_complete = false;
                }
                for step in &result.steps {
                    if let Some(coverage) = &step.coverage {
                        if coverage.files.is_empty() || coverage.baseline_matches == Some(false) {
                            scopes_complete = false;
                        }
                        scopes_complete &= add_coverage_files(
                            &mut lines_by_file,
                            &coverage.files,
                            &record.declared_sources,
                        );
                    } else {
                        scopes_complete = false;
                    }
                }
            }

            let files = record
                .declared_sources
                .iter()
                .map(|source| {
                    let executable = record
                        .line_coverage
                        .iter()
                        .find(|file| &file.path == source)
                        .map(|file| {
                            file.executable_lines
                                .iter()
                                .copied()
                                .collect::<BTreeSet<_>>()
                        })
                        .unwrap_or_default();
                    let covered = lines_by_file
                        .get(source)
                        .cloned()
                        .unwrap_or_default()
                        .intersection(&executable)
                        .copied()
                        .collect::<BTreeSet<_>>();
                    (source.clone(), executable, covered)
                })
                .collect::<Vec<_>>();
            let executable_lines = files
                .iter()
                .map(|(_, executable, _)| executable.len())
                .sum::<usize>();
            let covered_lines = files
                .iter()
                .map(|(_, _, covered)| covered.len())
                .sum::<usize>();
            let all_sources_present = record.declared_sources.iter().all(|source| {
                files
                    .iter()
                    .any(|(path, executable, _)| path == source && !executable.is_empty())
            });
            let coverage_complete = scopes_complete
                && record.missing_sources.is_empty()
                && all_sources_present
                && executable_lines > 0;
            let coverage_percent = coverage_complete.then(|| percent(covered_lines, executable_lines));
            let threshold_met = coverage_percent
                .is_some_and(|coverage| coverage + f64::EPSILON >= record.minimum_line_percent);
            let mut reasons = Vec::new();
            if !scopes_complete {
                reasons.push("cases lack nonempty coverage scopes or a declared fingerprint mismatched".to_string());
            }
            if !record.missing_sources.is_empty() {
                reasons.push(format!(
                    "missing declared sources: {}",
                    record.missing_sources.join(", ")
                ));
            }
            if !all_sources_present {
                reasons.push("no executable-line data for one or more declared sources".to_string());
            }
            if executable_lines == 0 {
                reasons.push("no target executable lines were reported".to_string());
            }
            json!({
                "target_id": target_id,
                "case_count": cases.len(),
                "passed_cases": cases.iter().filter(|result| result.passed).count(),
                "coverage_complete": coverage_complete,
                "coverage_percent": coverage_percent,
                "threshold_met": threshold_met,
                "covered_lines": covered_lines,
                "executable_lines": executable_lines,
                "incomplete_reason": if coverage_complete { None::<String> } else { Some(reasons.join("; ")) },
                "files": files.into_iter().map(|(path, executable, covered)| json!({
                    "path": path,
                    "executable_line_count": executable.len(),
                    "covered_line_count": covered.len(),
                    "uncovered_lines": executable.difference(&covered).copied().collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn target_coverage_thresholds_met(report: &BatchReport) -> bool {
    !report.coverage.is_empty()
        && report.coverage.iter().all(|record| {
            let results = report
                .results
                .iter()
                .filter(|result| result.contract == record.contract)
                .collect::<Vec<_>>();
            let targets = target_coverage_report(&results, record);
            !targets.is_empty() && targets.iter().all(|target| target["threshold_met"] == true)
        })
}

fn coverage_requirements_met(report: &BatchReport) -> bool {
    report.coverage_complete() && target_coverage_thresholds_met(report)
}

fn coverage_requirement_failures(report: &BatchReport) -> Vec<String> {
    let mut failures = Vec::new();
    for record in &report.coverage {
        if !record.line_threshold_met {
            failures.push(format!(
                "{} combined coverage: {:.1}% ({}/{}) below required {:.1}%",
                record.contract,
                percent(
                    record.combined_covered_lines,
                    record.combined_executable_lines
                ),
                record.combined_covered_lines,
                record.combined_executable_lines,
                record.minimum_line_percent
            ));
        }
        if !record.missing_sources.is_empty() {
            failures.push(format!(
                "{}: missing declared sources {}",
                record.contract,
                record.missing_sources.join(", ")
            ));
        }
        let results = report
            .results
            .iter()
            .filter(|result| result.contract == record.contract)
            .collect::<Vec<_>>();
        let targets = target_coverage_report(&results, record);
        if targets.is_empty() {
            failures.push(format!(
                "{}: no dataset targets were reported",
                record.contract
            ));
        }
        for target in targets
            .iter()
            .filter(|target| target["threshold_met"] != true)
        {
            let name = target["target_id"].as_str().unwrap_or("<unnamed>");
            if let (Some(covered), Some(executable), Some(coverage)) = (
                target["covered_lines"].as_u64(),
                target["executable_lines"].as_u64(),
                target["coverage_percent"].as_f64(),
            ) {
                failures.push(format!(
                    "{} target {}: {:.1}% ({}/{}) below required {:.1}%",
                    record.contract,
                    name,
                    coverage,
                    covered,
                    executable,
                    record.minimum_line_percent
                ));
            } else {
                failures.push(format!(
                    "{} target {}: coverage unavailable ({})",
                    record.contract,
                    name,
                    target["incomplete_reason"]
                        .as_str()
                        .unwrap_or("coverage evidence is incomplete")
                ));
            }
        }
    }
    if failures.is_empty() {
        failures.push("coverage evidence is incomplete".into());
    }
    failures
}

fn percent(covered: usize, executable: usize) -> f64 {
    if executable == 0 {
        0.0
    } else {
        covered as f64 * 100.0 / executable as f64
    }
}

fn concise_summary(report: &BatchReport) -> String {
    let mut output = format!(
        "Dataset cases: {}/{} passed across {} contract(s)\n",
        report.passed(),
        report.total(),
        report.coverage.len()
    );
    let value = serde_json::json!({
        "contracts": report.coverage.iter().map(|record| serde_json::json!({
            "contract": record.contract,
            "minimum_line_percent": record.minimum_line_percent,
            "target_coverage": target_coverage_report(
                &report.results.iter().filter(|result| result.contract == record.contract).collect::<Vec<_>>(),
                record,
            )
        })).collect::<Vec<_>>()
    });
    output.push_str(&coverage_line_summary(&value));
    for result in report.results.iter().filter(|result| !result.passed) {
        output.push_str(&format!(
            "  FAIL {}/{}/{}: {}\n",
            result.contract,
            result.target_id,
            result.case_id,
            result
                .error
                .as_deref()
                .unwrap_or("case verification failed")
        ));
        if let Some(actual) = &result.output {
            output.push_str(&format!("    actual: {actual}\n"));
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
    declared_sources: &[String],
) -> bool {
    let mut all_sources_resolved = true;
    for file in files {
        let Some(source) = resolve_declared_source(&file.path, declared_sources) else {
            all_sources_resolved = false;
            continue;
        };
        by_file
            .entry(source.to_owned())
            .or_default()
            .extend(file.lines.iter().copied());
    }
    all_sources_resolved
}

fn resolve_declared_source<'a>(
    scope_path: &str,
    declared_sources: &'a [String],
) -> Option<&'a str> {
    let mut matches = declared_sources
        .iter()
        .filter(|source| {
            source.as_str() == scope_path
                || source
                    .strip_suffix(scope_path)
                    .is_some_and(|prefix| prefix.ends_with('/'))
        })
        .map(String::as_str);
    let source = matches.next()?;
    matches.next().is_none().then_some(source)
}

#[cfg(test)]
mod tests {
    use super::{
        concise_summary, coverage_fingerprint_mismatches, coverage_line_summary,
        coverage_requirements_met, target_coverage_report,
    };
    use open_contract_core::batch::{
        CoverageFile, CoverageFingerprint, CoverageLineFile, CoverageRecord, StepResult,
    };
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
                "minimum_line_percent": 100,
                "target_coverage": [{
                    "target_id": "default",
                    "case_count": 1,
                    "passed_cases": 1,
                    "coverage_complete": true,
                    "coverage_percent": 62.5,
                    "covered_lines": 5,
                    "executable_lines": 8,
                    "files": [{
                        "path": "validator.py",
                        "executable_line_count": 8,
                        "covered_line_count": 5,
                        "uncovered_lines": [4, 5, 6]
                    }]
                }]
            }]
        });

        let summary = coverage_line_summary(&report);
        assert!(summary.contains("example target default: 1/1 cases passed; 5/8 target lines covered (62.5%, required 100.0%)"));
        assert!(summary.contains("validator.py: 5/8 target lines; uncovered lines 4-6"));
    }

    #[test]
    fn incomplete_target_coverage_never_prints_a_percentage() {
        let report = serde_json::json!({
            "contracts": [{
                "contract": "n1a-license-state",
                "minimum_line_percent": 100,
                "target_coverage": [{
                    "target_id": "n1a-license-state",
                    "case_count": 5,
                    "passed_cases": 0,
                    "coverage_complete": false,
                    "coverage_percent": null,
                    "incomplete_reason": "missing declared sources: license-state.js; cases lack nonempty coverage scopes",
                    "files": []
                }]
            }]
        });

        let summary = coverage_line_summary(&report);
        assert!(summary.contains("n1a-license-state target n1a-license-state: 0/5 cases passed; target coverage unavailable"));
        assert!(summary.contains("missing declared sources: license-state.js"));
        assert!(!summary.contains("100.0%"));
    }

    #[test]
    fn failed_case_summary_names_contract_target_case_and_actual_output() {
        let result = ContractResult {
            contract: "n1a-license-state".into(),
            target_id: "license-state".into(),
            case_id: "case-1".into(),
            output: Some(serde_json::json!({"active": false})),
            passed: false,
            matched_by: None,
            error: Some("output did not equal expected value".into()),
            steps: Vec::new(),
            coverage: Some(CoverageFingerprint {
                files: vec![CoverageFile {
                    path: "license-state.js".into(),
                    lines: vec![1],
                }],
                fingerprint: "observed".into(),
                expected_fingerprint: Some("expected".into()),
                baseline_matches: Some(true),
            }),
        };
        let report = BatchReport {
            results: vec![result],
            coverage: vec![CoverageRecord {
                contract: "n1a-license-state".into(),
                minimum_line_percent: 100.0,
                dataset_covered_lines: 1,
                dataset_executable_lines: 1,
                combined_covered_lines: 1,
                combined_executable_lines: 1,
                line_threshold_met: true,
                native_report: serde_json::json!({"sources": ["license-state.js"]}),
                declared_sources: vec!["license-state.js".into()],
                covered_sources: vec!["license-state.js".into()],
                missing_sources: Vec::new(),
                line_coverage: vec![CoverageLineFile {
                    path: "license-state.js".into(),
                    executable_lines: vec![1],
                    covered_lines: vec![1],
                }],
                additional_test_coverage: serde_json::Value::Null,
                additional_line_coverage: Vec::new(),
            }],
        };
        let summary = concise_summary(&report);
        assert!(summary.contains("Dataset cases: 0/1 passed across 1 contract(s)"));
        assert!(summary.contains("FAIL n1a-license-state/license-state/case-1"));
        assert!(summary.contains("actual: {\"active\":false}"));
    }

    #[test]
    fn target_scopes_below_threshold_fail_even_when_adapter_aggregate_passes() {
        let result = ContractResult {
            contract: "example".into(),
            target_id: "target".into(),
            case_id: "case".into(),
            output: Some(serde_json::json!({"ok": true})),
            passed: true,
            matched_by: None,
            error: None,
            steps: Vec::new(),
            coverage: Some(CoverageFingerprint {
                files: vec![CoverageFile {
                    path: "validator.js".into(),
                    lines: vec![1],
                }],
                fingerprint: "observed".into(),
                expected_fingerprint: None,
                baseline_matches: None,
            }),
        };
        let report = BatchReport {
            results: vec![result],
            coverage: vec![CoverageRecord {
                contract: "example".into(),
                minimum_line_percent: 100.0,
                dataset_covered_lines: 2,
                dataset_executable_lines: 2,
                combined_covered_lines: 2,
                combined_executable_lines: 2,
                line_threshold_met: true,
                native_report: serde_json::json!({"sources": ["validator.js"]}),
                declared_sources: vec!["validator.js".into()],
                covered_sources: vec!["validator.js".into()],
                missing_sources: Vec::new(),
                line_coverage: vec![CoverageLineFile {
                    path: "validator.js".into(),
                    executable_lines: vec![1, 2],
                    covered_lines: vec![1, 2],
                }],
                additional_test_coverage: serde_json::Value::Null,
                additional_line_coverage: Vec::new(),
            }],
        };

        assert!(report.coverage_complete());
        assert!(!coverage_requirements_met(&report));
        let target = target_coverage_report(
            &report.results.iter().collect::<Vec<_>>(),
            &report.coverage[0],
        );
        assert_eq!(target[0]["coverage_percent"], 50.0);
        assert_eq!(target[0]["threshold_met"], false);
    }

    #[test]
    fn target_coverage_ignores_lines_outside_executable_source() {
        let result = ContractResult {
            contract: "example".into(),
            target_id: "target".into(),
            case_id: "case".into(),
            output: None,
            passed: true,
            matched_by: None,
            error: None,
            steps: Vec::new(),
            coverage: Some(CoverageFingerprint {
                files: vec![CoverageFile {
                    path: "validator.js".into(),
                    lines: vec![1, 2, 99],
                }],
                fingerprint: "observed".into(),
                expected_fingerprint: None,
                baseline_matches: None,
            }),
        };
        let record = CoverageRecord {
            contract: "example".into(),
            minimum_line_percent: 100.0,
            dataset_covered_lines: 2,
            dataset_executable_lines: 2,
            combined_covered_lines: 2,
            combined_executable_lines: 2,
            line_threshold_met: true,
            native_report: serde_json::json!({"sources": ["validator.js"]}),
            declared_sources: vec!["validator.js".into()],
            covered_sources: vec!["validator.js".into()],
            missing_sources: Vec::new(),
            line_coverage: vec![CoverageLineFile {
                path: "validator.js".into(),
                executable_lines: vec![1, 2],
                covered_lines: vec![1, 2],
            }],
            additional_test_coverage: serde_json::Value::Null,
            additional_line_coverage: Vec::new(),
        };
        let target = target_coverage_report(&[&result], &record);
        assert_eq!(target[0]["covered_lines"], 2);
        assert_eq!(target[0]["executable_lines"], 2);
        assert_eq!(target[0]["coverage_percent"], 100.0);
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
