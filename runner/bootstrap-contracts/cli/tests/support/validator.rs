use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_verification::match_output;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) fn execute_dataset() -> Result<usize, String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let result = run_dataset(package.join("dataset.yaml"), |_, case| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        validate(case)?;
        #[cfg(coverage)]
        if capture_path.is_some() {
            snapshots.push((case.id.clone(), profile::snapshot()?));
        }
        Ok(())
    });
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)?;
        profile::write_snapshots(PathBuf::from(path).as_path(), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    result
}

#[cfg(coverage)]
#[path = "profile.rs"]
mod profile;

fn validate(case: &DatasetCase) -> Result<(), String> {
    let input = case.input.as_object().ok_or("input must be an object")?;
    if let Some(schema) = input.get("invalid_coverage_schema") {
        let schema = schema
            .as_str()
            .ok_or("invalid_coverage_schema must be a string")?;
        let actual = match crate::validate_coverage_report_with_schema(schema, &Value::Null) {
            Ok(()) => json!({ "valid": true }),
            Err(error) => json!({ "valid": false, "error": error.to_string() }),
        };
        return match_output(
            &actual,
            Some(case.expected_value()?),
            case.predicate.as_deref(),
        )
        .map(|_| ())
        .map_err(|error| {
            format!("coverage report schema compilation result did not match: {error}")
        });
    }
    if let Some(field) = input.get("invalid_coverage_report_field") {
        let field = field
            .as_str()
            .ok_or("invalid_coverage_report_field must be a string")?;
        let mut report = valid_coverage_report();
        match field {
            "dataset-object" => report["contracts"][0]["union"] = json!([]),
            "duplicate-lines" => {
                report["contracts"][0]["union"]["files"][0]["executable_lines"] = json!([1, 1]);
            }
            "zero-line" => {
                report["contracts"][0]["union"]["files"][0]["executable_lines"] = json!([0]);
            }
            other => return Err(format!("unknown invalid coverage report field: {other}")),
        }
        let actual = match crate::validate_coverage_report(&report) {
            Ok(()) => json!({ "valid": true }),
            Err(error) => json!({ "valid": false, "error": error.to_string() }),
        };
        return match_output(
            &actual,
            Some(case.expected_value()?),
            case.predicate.as_deref(),
        )
        .map(|_| ())
        .map_err(|error| format!("coverage report schema result did not match: {error}"));
    }
    if let Some(report) = input.get("validate_coverage_report") {
        let actual = match crate::validate_coverage_report(report) {
            Ok(()) => json!({ "valid": true }),
            Err(error) => json!({ "valid": false, "error": error.to_string() }),
        };
        return match_output(
            &actual,
            Some(case.expected_value()?),
            case.predicate.as_deref(),
        )
        .map(|_| ())
        .map_err(|error| format!("coverage report schema result did not match: {error}"));
    }
    if let Some(fixture) = input.get("write_invalid_coverage_report") {
        use runner_batch::{CoverageMetricOutcome, CoverageRecord};

        let minimum_coverage_percent = fixture
            .get("minimum_coverage_percent")
            .and_then(Value::as_f64)
            .ok_or("write_invalid_coverage_report.minimum_coverage_percent must be a number")?;
        let metric_outcomes: std::collections::BTreeMap<String, CoverageMetricOutcome> =
            serde_json::from_value(sample_metrics(false))
                .map_err(|error| format!("invalid sample metric outcomes: {error}"))?;
        let report = runner_batch::BatchReport {
            results: Vec::new(),
            coverage: vec![CoverageRecord {
                contract: "invalid-report-fixture".into(),
                dataset_paths: vec![PathBuf::from("dataset.json")],
                datasets: vec!["dataset.json".into()],
                minimum_coverage_percent,
                dataset_covered_lines: 0,
                dataset_executable_lines: 0,
                combined_covered_lines: 0,
                combined_executable_lines: 0,
                line_threshold_met: false,
                coverage_threshold_met: false,
                dataset_metrics: metric_outcomes.clone(),
                aggregate_metrics: metric_outcomes,
                native_report: Value::Null,
                declared_sources: Vec::new(),
                covered_sources: Vec::new(),
                missing_sources: Vec::new(),
                line_coverage: Vec::new(),
                additional_test_coverage: Value::Null,
                additional_line_coverage: Vec::new(),
            }],
        };
        let actual = match crate::write_adapter_coverage_reports(
            Path::new("must-not-be-written.json"),
            &report,
        ) {
            Ok(()) => json!({ "written": true }),
            Err(error) => json!({ "written": false, "error": error.to_string() }),
        };
        return match_output(
            &actual,
            Some(case.expected_value()?),
            case.predicate.as_deref(),
        )
        .map(|_| ())
        .map_err(|error| format!("invalid report writer result did not match: {error}"));
    }
    let arguments = input
        .get("args")
        .and_then(Value::as_array)
        .ok_or("input.args must be an array")?;
    let arguments = arguments
        .iter()
        .map(|argument| {
            argument
                .as_str()
                .map(str::to_owned)
                .ok_or("each argument must be a string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let files = input.get("files").and_then(Value::as_object);
    let has_environment = input.get("environment").is_some();
    let needs_fixture = files.is_some() || has_environment;
    let expected_setup = needs_fixture.then_some(case.id.as_str());
    if case.setup.as_deref() != expected_setup {
        return Err(
            "setup must name the case fixture exactly when CLI filesystem or environment input is supplied"
                .into(),
        );
    }
    let temporary = if case.setup.is_some() {
        Some(
            tempfile::tempdir()
                .map_err(|error| format!("cannot create CLI working directory: {error}"))?,
        )
    } else {
        None
    };
    let original_directory = std::env::current_dir()
        .map_err(|error| format!("cannot read current directory: {error}"))?;
    if let Some(directory) = &temporary {
        for (relative, contents) in files.into_iter().flatten() {
            let relative = Path::new(relative);
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(format!(
                    "input file path must stay within the CLI working directory: {}",
                    relative.display()
                ));
            }
            let path = directory.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create input directory: {error}"))?;
            }
            let contents = contents
                .as_str()
                .ok_or("input.files values must be strings")?;
            fs::write(path, contents)
                .map_err(|error| format!("cannot write CLI input file: {error}"))?;
        }
        std::env::set_current_dir(directory.path())
            .map_err(|error| format!("cannot set CLI working directory: {error}"))?;
    }
    let actual = {
        let _environment = CliEnvironmentGuard::apply(
            input.get("environment"),
            case.setup.as_deref().unwrap_or(case.id.as_str()),
            temporary.as_ref().map(tempfile::TempDir::path),
        )?;
        crate::execute(&arguments)
    };
    std::env::set_current_dir(original_directory)
        .map_err(|error| format!("cannot restore current directory: {error}"))?;
    let metric_summary = actual
        .stdout
        .lines()
        .filter(|line| line.starts_with("    Coverage metrics:"))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized_stdout = without_coverage_metric_summary(&actual.stdout);
    let mut actual_value = json!({
        "exit_code": actual.exit_code,
        "stdout": normalized_stdout,
        "stderr": actual.stderr,
    });
    let mut expected = case.expected_value()?.clone();
    if let Some(expected_summary) = expected
        .as_object_mut()
        .and_then(|expected| expected.remove("coverage_metrics_summary"))
    {
        match_output(
            &Value::String(metric_summary),
            Some(&expected_summary),
            None,
        )
        .map_err(|error| format!("coverage metric CLI summary did not match: {error}"))?;
    }
    if let Some(stdout) = expected
        .as_object_mut()
        .and_then(|expected| expected.get_mut("stdout"))
        .and_then(|value| value.as_str().map(without_coverage_metric_summary))
    {
        expected["stdout"] = Value::String(stdout);
    }
    if expected.get("coverage_report").is_some() {
        let report_arg = arguments
            .windows(2)
            .find(|pair| pair[0] == "--coverage-report")
            .map(|pair| pair[1].as_str())
            .unwrap_or(".coverage/latest.json");
        let report_path = temporary.as_ref().map_or_else(
            || PathBuf::from(report_arg),
            |directory| directory.path().join(report_arg),
        );
        let report_bytes = fs::read(&report_path).map_err(|error| {
            format!(
                "cannot read CLI coverage report {}: {error}",
                report_path.display()
            )
        })?;
        let report: Value = serde_json::from_slice(&report_bytes)
            .map_err(|error| format!("CLI coverage report is not JSON: {error}"))?;
        actual_value["coverage_report"] = report;
    }
    if expected.get("updated_dataset").is_some() {
        let dataset_path = input
            .get("dataset_path")
            .and_then(Value::as_str)
            .ok_or("updated_dataset expectation requires input.dataset_path")?;
        let dataset_path = temporary.as_ref().map_or_else(
            || PathBuf::from(dataset_path),
            |directory| directory.path().join(dataset_path),
        );
        let dataset_bytes = fs::read(&dataset_path).map_err(|error| {
            format!(
                "cannot read updated dataset {}: {error}",
                dataset_path.display()
            )
        })?;
        let dataset: Value = if dataset_path
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("json")
        {
            serde_json::from_slice(&dataset_bytes)
                .map_err(|error| format!("updated dataset is not JSON: {error}"))?
        } else {
            let yaml: serde_yaml::Value = serde_yaml::from_slice(&dataset_bytes)
                .map_err(|error| format!("updated dataset is not YAML: {error}"))?;
            serde_json::to_value(yaml)
                .map_err(|error| format!("cannot convert updated dataset: {error}"))?
        };
        actual_value["updated_dataset"] = dataset;
    }
    match_output(&actual_value, Some(&expected), case.predicate.as_deref()).map_err(|error| {
        format!("complete CLI result did not match: {error}; actual: {actual_value}")
    })?;
    Ok(())
}

fn without_coverage_metric_summary(output: &str) -> String {
    let mut normalized = output
        .lines()
        .filter(|line| !line.starts_with("    Coverage metrics:"))
        .collect::<Vec<_>>()
        .join("\n");
    if output.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn valid_coverage_report() -> Value {
    json!({
        "version": 1,
        "contracts": [{
            "contract": "schema-fixture",
            "status": "passed",
            "minimum_coverage_percent": 100,
            "datasets": ["dataset.json"],
            "sources": {
                "status": "complete",
                "declared": ["src/validator.py"],
                "covered": ["src/validator.py"],
                "missing": []
            },
            "union": {
                "covered_lines": 1,
                "executable_lines": 1,
                "percent": 100,
                "metrics": sample_metrics(true),
                "files": [{
                    "path": "src/validator.py",
                    "executable_lines": [1],
                    "covered_lines": [1],
                    "uncovered_lines": []
                }]
            },
            "aggregate": {
                "covered_lines": 1,
                "executable_lines": 1,
                "percent": 100,
                "threshold_met": true,
                "metrics": sample_metrics(true),
                "files": [{
                    "path": "src/validator.py",
                    "executable_lines": [1],
                    "covered_lines": [1],
                    "uncovered_lines": []
                }]
            },
            "supplemental_tests": null,
            "native_report": null
        }]
    })
}

fn sample_metrics(line_passes: bool) -> Value {
    let mut metrics = serde_json::Map::new();
    for name in [
        "line",
        "statement",
        "function",
        "branch",
        "region",
        "instantiation",
    ] {
        let outcome = if name == "line" {
            json!({
                "status": "available",
                "covered": 1,
                "total": 1,
                "percent": 100,
                "minimum_percent": 100,
                "threshold_met": line_passes,
                "files": [{"path":"src/validator.py", "covered":1, "total":1}]
            })
        } else {
            json!({
                "status": "unavailable",
                "covered": null,
                "total": null,
                "percent": null,
                "minimum_percent": null,
                "threshold_met": null,
                "files": []
            })
        };
        metrics.insert(name.to_owned(), outcome);
    }
    Value::Object(metrics)
}

struct CliEnvironmentGuard {
    previous: Vec<(String, Option<std::ffi::OsString>)>,
}

impl CliEnvironmentGuard {
    fn apply(
        environment: Option<&Value>,
        case_id: &str,
        temporary: Option<&Path>,
    ) -> Result<Self, String> {
        let updates = environment
            .map(|value| {
                value
                    .as_object()
                    .ok_or_else(|| "input.environment must be an object".to_string())?
                    .iter()
                    .map(|(name, value)| {
                        value
                            .as_str()
                            .map(|value| (name.clone(), value.to_owned()))
                            .ok_or_else(|| format!("input.environment.{name} must be a string"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let mut names = vec![
            "OPEN_CONTRACT_ISOLATION".to_owned(),
            "OPEN_CONTRACT_DISABLE_FINGERPRINTS".to_owned(),
        ];
        if matches!(
            case_id,
            "run-manifest-defaults-to-auto-isolation"
                | "run-manifest-auto-prefers-bwrap"
                | "run-manifest-auto-falls-back-to-none"
        ) {
            names.push("PATH".to_owned());
        }
        if case_id == "run-manifest-auto-falls-back-to-none" {
            names.push("TMPDIR".to_owned());
        }
        for (name, _) in &updates {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        let previous = names
            .iter()
            .map(|name| (name.clone(), std::env::var_os(name)))
            .collect::<Vec<_>>();
        for name in &names {
            std::env::remove_var(name);
        }
        for (name, value) in updates {
            std::env::set_var(name, value);
        }
        if matches!(
            case_id,
            "run-manifest-defaults-to-auto-isolation" | "run-manifest-auto-prefers-bwrap"
        ) {
            let root = temporary.ok_or("Bubblewrap fixture requires a CLI temporary directory")?;
            let launcher = root.join("bin/bwrap");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = fs::metadata(&launcher)
                    .map_err(|error| format!("cannot inspect Bubblewrap fixture: {error}"))?
                    .permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&launcher, permissions).map_err(|error| {
                    format!("cannot make Bubblewrap fixture executable: {error}")
                })?;
            }
            let original = previous
                .iter()
                .find(|(name, _)| name == "PATH")
                .and_then(|(_, value)| value.clone())
                .unwrap_or_default();
            let mut paths = vec![root.join("bin")];
            paths.extend(std::env::split_paths(&original));
            let path = std::env::join_paths(paths).map_err(|error| error.to_string())?;
            std::env::set_var("PATH", path);
        }
        if case_id == "run-manifest-auto-falls-back-to-none" {
            let root = temporary.ok_or("none fallback requires a CLI temporary directory")?;
            let blocker = root.join("tmp-blocker");
            if !blocker.is_file() {
                return Err("none fallback input must create tmp-blocker".into());
            }
            std::env::set_var("TMPDIR", blocker);
        }
        Ok(Self { previous })
    }
}

impl Drop for CliEnvironmentGuard {
    fn drop(&mut self) {
        for (name, value) in self.previous.drain(..) {
            if let Some(value) = value {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}
