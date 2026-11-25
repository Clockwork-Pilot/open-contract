use runner_batch::{
    has_open_contract_filename, run_contract_tree_with_fingerprint_mode_and_nested_contracts,
    show_isolation_plan, BatchReport, FingerprintMode, ManifestBatchAdapter,
};
use runner_errors::ContractError;
use runner_isolation::{auto_backend, Backend};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Eq, PartialEq)]
pub struct CliOutput {
    pub exit_code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Execute the Open Contract Rust CLI with argument strings excluding argv[0].
/// The binary and bootstrap dataset both call this public entry point.
pub fn execute(arguments: &[String]) -> CliOutput {
    let mut stdout = String::new();
    match run(arguments.iter().cloned(), &mut stdout) {
        Ok(()) => CliOutput {
            exit_code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            exit_code: error.exit_code(),
            stdout,
            stderr: format!("contract-runner: {error}\n"),
        },
    }
}

fn run(mut args: impl Iterator<Item = String>, stdout: &mut String) -> Result<(), ContractError> {
    let subcommand = args.next();
    if subcommand.as_deref() == Some("show-isolation-plan") {
        stdout.push_str(&show_plan(args)?);
        return Ok(());
    }
    if subcommand.as_deref() != Some("run-manifest") {
        return Err((2, "usage: contract-runner <run-manifest|show-isolation-plan> <manifest-path> [--isolation <auto|tempdir|none|bwrap|landlock>] [--isolation-runtime <path>] [--coverage-report <path>] [--disable-fingerprints|--update-fingerprints] [--no-nested-contracts]".into()).into());
    }
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    if !has_open_contract_filename(&manifest) {
        return Err((
            2,
            "run-manifest manifest filename must contain a dot-delimited open-contract segment before .yaml or .yml".into(),
        )
            .into());
    }
    let mut backend = None;
    let mut isolation_runtime = None;
    let mut coverage_report = PathBuf::from(".coverage/latest.json");
    let mut disable_fingerprints = false;
    let mut update_fingerprints = false;
    let mut include_nested_contracts = true;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_isolation_choice(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?)
            }
            "--coverage-report" => {
                coverage_report = PathBuf::from(
                    args.next()
                        .ok_or((2, "missing coverage report path".into()))?,
                );
            }
            "--isolation-runtime" => {
                isolation_runtime = Some(PathBuf::from(
                    args.next()
                        .ok_or((2, "missing isolation runtime path".into()))?,
                ));
            }
            "--disable-fingerprints" => disable_fingerprints = true,
            "--update-fingerprints" => update_fingerprints = true,
            "--no-nested-contracts" => include_nested_contracts = false,
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    if disable_fingerprints && update_fingerprints {
        return Err((
            2,
            "--update-fingerprints cannot be combined with --disable-fingerprints".into(),
        )
            .into());
    }
    let choice = match backend {
        Some(backend) => backend,
        None => isolation_from_environment()?.unwrap_or(IsolationChoice::Auto),
    };
    let (backend, auto_attempts) = resolve_isolation_choice(choice, &mut isolation_runtime);
    write_auto_selection(stdout, backend, auto_attempts.as_deref());
    let environment_disables_fingerprints = fingerprints_disabled_by_environment()?;
    if update_fingerprints && environment_disables_fingerprints {
        return Err((
            2,
            "--update-fingerprints cannot be used when OPEN_CONTRACT_DISABLE_FINGERPRINTS=1".into(),
        )
            .into());
    }
    let fingerprint_mode = if update_fingerprints {
        FingerprintMode::Update
    } else if disable_fingerprints || environment_disables_fingerprints {
        FingerprintMode::Disabled
    } else {
        FingerprintMode::Check
    };
    let coverage_fingerprints = fingerprint_mode != FingerprintMode::Disabled;
    if matches!(backend, Backend::None | Backend::TempDir) && isolation_runtime.is_some() {
        let isolation_name = if backend == Backend::TempDir {
            "tempdir"
        } else {
            "none"
        };
        return Err((
            2,
            format!(
                "--isolation-runtime cannot be used with --isolation {}",
                isolation_name
            ),
        )
            .into());
    }
    let report = run_contract_tree_with_fingerprint_mode_and_nested_contracts(
        manifest,
        backend,
        isolation_runtime,
        fingerprint_mode,
        include_nested_contracts,
        &ManifestBatchAdapter,
    )
    .map_err(|error| ContractError::from((2, error)).context("contract execution"))?;
    write_adapter_coverage_reports(&coverage_report, &report)?;
    if update_fingerprints
        && report_passed(&report) == report_total(&report)
        && report_coverage_complete(&report, coverage_fingerprints)
    {
        let updates = update_dataset_fingerprints(&report)?;
        stdout.push_str(&updates);
    } else if update_fingerprints {
        stdout.push_str(
            "Fingerprints were not updated because dataset cases or coverage requirements failed\n",
        );
    }
    if !coverage_fingerprints {
        stdout.push_str("Coverage fingerprints: disabled\n");
    }
    stdout.push_str(&concise_summary(&report));
    stdout.push_str(&format!("Coverage report: {}\n", coverage_report.display()));
    if report_passed(&report) != report_total(&report) {
        return Err((
            1,
            format!(
                "{}/{} dataset cases passed across {} contract(s)",
                report_passed(&report),
                report_total(&report),
                report.coverage.len()
            ),
        )
            .into());
    }
    if !report_coverage_complete(&report, coverage_fingerprints) {
        return Err((
            1,
            "coverage requirements not met; see adapter coverage report".into(),
        )
            .into());
    }
    Ok(())
}

fn show_plan(mut args: impl Iterator<Item = String>) -> Result<String, ContractError> {
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    let mut backend = None;
    let mut isolation_runtime = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_isolation_choice(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?);
            }
            "--isolation-runtime" => {
                isolation_runtime = Some(PathBuf::from(
                    args.next()
                        .ok_or((2, "missing isolation runtime path".into()))?,
                ));
            }
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    let choice = match backend {
        Some(backend) => backend,
        None => isolation_from_environment()?.unwrap_or(IsolationChoice::Auto),
    };
    let (backend, auto_attempts) = resolve_isolation_choice(choice, &mut isolation_runtime);
    if matches!(backend, Backend::None | Backend::TempDir) && isolation_runtime.is_some() {
        let isolation_name = if backend == Backend::TempDir {
            "tempdir"
        } else {
            "none"
        };
        return Err((
            2,
            format!(
                "--isolation-runtime cannot be used with --isolation {}",
                isolation_name
            ),
        )
            .into());
    }
    let mut output = show_isolation_plan(manifest, backend, isolation_runtime)
        .map_err(|error| ContractError::from((2, error)).context("isolation plan"))?;
    write_auto_selection(&mut output, backend, auto_attempts.as_deref());
    Ok(output)
}

fn write_auto_selection(output: &mut String, backend: Backend, attempts: Option<&[String]>) {
    let Some(attempts) = attempts else {
        output.push_str(&format!("Isolation mode: {}\n", backend_name(backend)));
        return;
    };
    output.push_str(&format!(
        "Isolation auto selected: {}\n",
        backend_name(backend)
    ));
    for attempt in attempts {
        output.push_str(&format!("  {attempt}\n"));
    }
}

/// Persist the normalized, versioned runner artifact and retain native adapter reports.
pub fn write_adapter_coverage_reports(
    path: &std::path::Path,
    report: &BatchReport,
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
            let dataset_coverage = coverage_totals_json(
                record.dataset_covered_lines,
                record.dataset_executable_lines,
                &record.line_coverage,
                None,
            );
            let aggregate_coverage = coverage_totals_json(
                record.combined_covered_lines,
                record.combined_executable_lines,
                &merge_coverage_files(&record.line_coverage, &record.additional_line_coverage),
                Some(record.line_threshold_met),
            );
            let source_status = if record.missing_sources.is_empty() {
                "complete"
            } else {
                "missing"
            };
            let status = if record.missing_sources.is_empty()
                && record.line_threshold_met
                && record.additional_test_coverage["passed"] != false
            {
                "passed"
            } else {
                "failed"
            };
            json!({
                "contract": record.contract,
                "status": status,
                "minimum_line_percent": record.minimum_line_percent,
                "sources": {
                    "status": source_status,
                    "declared": record.declared_sources,
                    "covered": record.covered_sources,
                    "missing": record.missing_sources,
                },
                "dataset": dataset_coverage,
                "aggregate": aggregate_coverage,
                "supplemental_tests": record.additional_test_coverage,
                "native_report": record.native_report,
            })
        })
        .collect::<Vec<Value>>();
    let artifact = json!({ "version": 1, "contracts": contracts });
    validate_coverage_report(&artifact)?;
    let contents = artifact.to_string();
    fs::write(path, contents).map_err(|error| {
        ContractError::from((
            2,
            format!("cannot write coverage report {}: {error}", path.display()),
        ))
    })
}

/// Validate a versioned coverage artifact before a consumer stores or processes it.
pub fn validate_coverage_report(report: &Value) -> Result<(), ContractError> {
    validate_coverage_report_with_schema(
        include_str!("../../../../schemas/coverage-report.v1.schema.json"),
        report,
    )
}

fn validate_coverage_report_with_schema(
    schema_json: &str,
    report: &Value,
) -> Result<(), ContractError> {
    let schema: Value = serde_json::from_str(schema_json).map_err(|error| {
        ContractError::from((
            2,
            format!("invalid embedded coverage report schema: {error}"),
        ))
    })?;
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        ContractError::from((
            2,
            format!("cannot compile embedded coverage report schema: {error}"),
        ))
    })?;
    validator.validate(report).map_err(|error| {
        ContractError::from((
            2,
            format!(
                "invalid coverage report at {}: {error}",
                error.instance_path()
            ),
        ))
    })
}

fn coverage_totals_json(
    covered: usize,
    executable: usize,
    files: &[runner_batch::CoverageLineFile],
    threshold_met: Option<bool>,
) -> Value {
    let percent = if executable == 0 {
        0.0
    } else {
        covered as f64 * 100.0 / executable as f64
    };
    let mut value = json!({
        "covered_lines": covered,
        "executable_lines": executable,
        "percent": percent,
        "files": files.iter().map(|file| coverage_file_json(file)).collect::<Vec<_>>(),
    });
    if let Some(threshold_met) = threshold_met {
        value["threshold_met"] = json!(threshold_met);
    }
    value
}

fn coverage_file_json(file: &runner_batch::CoverageLineFile) -> Value {
    let executable = file
        .executable_lines
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let covered = file
        .covered_lines
        .iter()
        .copied()
        .filter(|line| executable.contains(line))
        .collect::<std::collections::BTreeSet<_>>();
    let uncovered = executable.difference(&covered).copied().collect::<Vec<_>>();
    json!({
        "path": file.path,
        "executable_lines": executable.into_iter().collect::<Vec<_>>(),
        "covered_lines": covered.into_iter().collect::<Vec<_>>(),
        "uncovered_lines": uncovered,
    })
}

fn merge_coverage_files(
    dataset: &[runner_batch::CoverageLineFile],
    supplemental: &[runner_batch::CoverageLineFile],
) -> Vec<runner_batch::CoverageLineFile> {
    let mut files = std::collections::BTreeMap::<
        String,
        (
            std::collections::BTreeSet<u32>,
            std::collections::BTreeSet<u32>,
        ),
    >::new();
    for file in dataset.iter().chain(supplemental) {
        let (executable, covered) = files.entry(file.path.clone()).or_default();
        executable.extend(file.executable_lines.iter().copied());
        covered.extend(file.covered_lines.iter().copied());
    }
    files
        .into_iter()
        .map(
            |(path, (executable, covered))| runner_batch::CoverageLineFile {
                path,
                executable_lines: executable.into_iter().collect(),
                covered_lines: covered.into_iter().collect(),
            },
        )
        .collect()
}

fn update_dataset_fingerprints(report: &BatchReport) -> Result<String, ContractError> {
    let mut output = String::new();
    for coverage in &report.coverage {
        let mut pins = Vec::new();
        for result in report
            .results
            .iter()
            .filter(|result| result.contract == coverage.contract)
        {
            if result.steps.is_empty() {
                pins.extend(result.coverage.as_ref().map(|fingerprint| {
                    (
                        result.case_id.as_str(),
                        None,
                        fingerprint.fingerprint.as_str(),
                        fingerprint.files.clone(),
                    )
                }));
            }
            pins.extend(result.steps.iter().filter_map(|step| {
                step.coverage.as_ref().map(|fingerprint| {
                    (
                        result.case_id.as_str(),
                        Some(step.id.as_str()),
                        fingerprint.fingerprint.as_str(),
                        fingerprint.files.clone(),
                    )
                })
            }));
        }
        let dataset_path = &coverage.dataset_path;
        let source = fs::read(dataset_path).map_err(|error| {
            ContractError::from((
                2,
                format!("cannot read dataset {}: {error}", dataset_path.display()),
            ))
        })?;
        let mut dataset: serde_yaml::Value =
            serde_yaml::from_slice(&source).map_err(yaml_dataset_error)?;
        let cases = dataset
            .as_mapping_mut()
            .and_then(|root| root.get_mut(serde_yaml::Value::String("cases".into())))
            .and_then(serde_yaml::Value::as_sequence_mut)
            .ok_or_else(|| {
                ContractError::from((
                    2,
                    format!("dataset has no cases array: {}", dataset_path.display()),
                ))
            })?;
        let mut changed = Vec::new();
        for (case_id, step_id, observed, files) in pins {
            let case = cases
                .iter_mut()
                .find(|case| mapping_string(case, "id") == Some(case_id))
                .ok_or_else(|| {
                    ContractError::from((
                        2,
                        format!("case {case_id} not found in {}", dataset_path.display()),
                    ))
                })?;
            let target = if let Some(step_id) = step_id {
                case.as_mapping_mut()
                    .and_then(|mapping| mapping.get_mut(serde_yaml::Value::String("steps".into())))
                    .and_then(serde_yaml::Value::as_sequence_mut)
                    .and_then(|steps| {
                        steps
                            .iter_mut()
                            .find(|step| mapping_string(step, "id") == Some(step_id))
                    })
                    .ok_or_else(|| {
                        ContractError::from((
                            2,
                            format!(
                                "step {case_id}/{step_id} not found in {}",
                                dataset_path.display()
                            ),
                        ))
                    })?
            } else {
                case
            };
            // The target was located by its string `id` above, so it is a mapping.
            let old = target["coverage_fingerprint"]
                .as_str()
                .unwrap_or("<missing>")
                .to_owned();
            if old != observed {
                target["coverage_fingerprint"] = serde_yaml::Value::String(observed.to_owned());
                changed.push((
                    case_id.to_owned(),
                    step_id.map(str::to_owned),
                    old,
                    observed.to_owned(),
                    files,
                ));
            }
        }
        if changed.is_empty() {
            continue;
        }
        // JSON is also valid YAML, so one serializer preserves parseability for
        // both supported dataset extensions while keeping refresh output stable.
        let serialized = serde_json::to_string_pretty(&dataset).map_err(json_dataset_error)? + "\n";
        fs::write(dataset_path, serialized).map_err(|error| {
            ContractError::from((
                2,
                format!("cannot update dataset {}: {error}", dataset_path.display()),
            ))
        })?;
        for (case_id, step_id, old, observed, files) in &changed {
            let subject = step_id
                .as_ref()
                .map(|step| format!("{case_id}/{step}"))
                .unwrap_or_else(|| case_id.clone());
            output.push_str(&format!(
                "Updated {}/{} fingerprint: {} -> {}\n",
                coverage.contract, subject, old, observed
            ));
            output.push_str(&format!("  covered: {}\n", format_covered_lines(files)));
        }
        output.push_str(&format!(
            "Updated {} fingerprint(s) in {}\n",
            changed.len(),
            dataset_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("dataset")
        ));
    }
    if output.is_empty() {
        output.push_str("Fingerprints already match; no dataset changes made\n");
    }
    Ok(output)
}

fn yaml_dataset_error(error: serde_yaml::Error) -> ContractError {
    ContractError::from((2, format!("YAML dataset error: {error}")))
}

fn json_dataset_error(error: serde_json::Error) -> ContractError {
    ContractError::from((2, format!("JSON dataset error: {error}")))
}

fn format_covered_lines(files: &[runner_batch::CoverageFile]) -> String {
    files
        .iter()
        .map(|file| {
            let lines = file
                .lines
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:{lines}", file.path)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn mapping_string<'a>(value: &'a serde_yaml::Value, key: &str) -> Option<&'a str> {
    value
        .as_mapping()?
        .get(serde_yaml::Value::String(key.into()))?
        .as_str()
}

fn concise_summary(report: &BatchReport) -> String {
    let mut output = String::from("Contracts executed:\n");
    for contract in &report.coverage {
        let results = report
            .results
            .iter()
            .filter(|result| result.contract == contract.contract);
        let (passed, total) = results.fold((0, 0), |(passed, total), result| {
            (passed + usize::from(result.passed), total + 1)
        });
        output.push_str(&format!(
            "  {}: {passed}/{total} cases passed\n",
            contract.contract
        ));
    }
    output.push_str(&format!(
        "Dataset cases: {}/{} passed across {} contract(s)\n",
        report_passed(report),
        report_total(report),
        report.coverage.len()
    ));
    for result in report.results.iter().filter(|result| !result.passed) {
        output.push_str(&format!(
            "  FAIL {}/{}: {}\n",
            result.contract,
            result.case_id,
            result
                .error
                .as_deref()
                .unwrap_or("case verification failed")
        ));
        output.push_str(&format!(
            "    actual: {}\n",
            result.output.as_ref().unwrap_or(&Value::Null)
        ));
    }
    output
}

fn report_passed(report: &BatchReport) -> usize {
    report.results.iter().filter(|result| result.passed).count()
}

fn report_total(report: &BatchReport) -> usize {
    report.results.len()
}

fn report_coverage_complete(report: &BatchReport, coverage_fingerprints: bool) -> bool {
    !report.coverage.is_empty()
        && report.coverage.iter().all(|record| {
            record.missing_sources.is_empty()
                && record.additional_test_coverage["passed"] != false
                && record.line_threshold_met
        })
        && !report.results.is_empty()
        && (!coverage_fingerprints
            || report.results.iter().all(|result| {
                let valid_scope = |coverage: &Option<runner_batch::CoverageFingerprint>| {
                    coverage.as_ref().is_some_and(|coverage| {
                        !coverage.files.is_empty() && coverage.baseline_matches != Some(false)
                    })
                };
                valid_scope(&result.coverage)
                    && result.steps.iter().all(|step| valid_scope(&step.coverage))
            }))
}

#[derive(Clone, Copy)]
enum IsolationChoice {
    Auto,
    Explicit(Backend),
}

fn isolation_from_environment() -> Result<Option<IsolationChoice>, ContractError> {
    std::env::var("OPEN_CONTRACT_ISOLATION")
        .ok()
        .map(|value| parse_isolation_choice(&value))
        .transpose()
}

fn resolve_isolation_choice(
    choice: IsolationChoice,
    isolation_runtime: &mut Option<PathBuf>,
) -> (Backend, Option<Vec<String>>) {
    match choice {
        IsolationChoice::Auto => {
            let (backend, mut attempts) = auto_backend(isolation_runtime.as_deref());
            if matches!(backend, Backend::TempDir | Backend::None) && isolation_runtime.is_some() {
                *isolation_runtime = None;
                attempts
                    .push("custom isolation runtime is not used by the selected fallback".into());
            }
            (backend, Some(attempts))
        }
        IsolationChoice::Explicit(backend) => (backend, None),
    }
}

fn fingerprints_disabled_by_environment() -> Result<bool, ContractError> {
    let Some(value) = std::env::var("OPEN_CONTRACT_DISABLE_FINGERPRINTS").ok() else {
        return Ok(false);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err((
            2,
            "OPEN_CONTRACT_DISABLE_FINGERPRINTS must be 0/1, true/false, yes/no, or on/off".into(),
        )
            .into()),
    }
}

fn parse_isolation_choice(value: &str) -> Result<IsolationChoice, ContractError> {
    match value {
        "auto" => Ok(IsolationChoice::Auto),
        "tempdir" => Ok(IsolationChoice::Explicit(Backend::TempDir)),
        "none" => Ok(IsolationChoice::Explicit(Backend::None)),
        "bwrap" => Ok(IsolationChoice::Explicit(Backend::Bubblewrap)),
        "landlock" => Ok(IsolationChoice::Explicit(Backend::Landlock)),
        _ => Err((
            2,
            format!(
                "invalid isolation backend {value:?}; expected auto, tempdir, none, bwrap, or landlock"
            ),
        )
            .into()),
    }
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::TempDir => "tempdir",
        Backend::None => "none",
        Backend::Bubblewrap => "bwrap",
        Backend::Landlock => "landlock",
    }
}

#[cfg(test)]
#[path = "../tests/unit_dataset_runner.rs"]
mod dataset_runner_test;
