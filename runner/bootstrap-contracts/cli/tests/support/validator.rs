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
    let arguments = input.get("args").and_then(Value::as_array).ok_or("input.args must be an array")?;
    let arguments = arguments.iter().map(|argument| argument.as_str().map(str::to_owned).ok_or("each argument must be a string")).collect::<Result<Vec<_>, _>>()?;
    let files = input.get("files").and_then(Value::as_object);
    if case.setup != files.is_some() {
        return Err("setup must be true exactly when input.files supplies CLI filesystem inputs".into());
    }
    let temporary = if case.setup {
        Some(tempfile::tempdir().map_err(|error| format!("cannot create CLI working directory: {error}"))?)
    } else {
        None
    };
    let original_directory = std::env::current_dir().map_err(|error| format!("cannot read current directory: {error}"))?;
    if let Some(directory) = &temporary {
        for (relative, contents) in files.into_iter().flatten() {
            let relative = Path::new(relative);
            if relative.as_os_str().is_empty() || relative.is_absolute()
                || relative.components().any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(format!("input file path must stay within the CLI working directory: {}", relative.display()));
            }
            let path = directory.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("cannot create input directory: {error}"))?;
            }
            let contents = contents.as_str().ok_or("input.files values must be strings")?;
            fs::write(path, contents).map_err(|error| format!("cannot write CLI input file: {error}"))?;
        }
        std::env::set_current_dir(directory.path()).map_err(|error| format!("cannot set CLI working directory: {error}"))?;
    }
    let actual = crate::execute(&arguments);
    std::env::set_current_dir(original_directory).map_err(|error| format!("cannot restore current directory: {error}"))?;
    let mut actual_value = json!({
        "exit_code": actual.exit_code,
        "stdout": actual.stdout,
        "stderr": actual.stderr,
    });
    let expected = case.expected.as_ref();
    if expected.is_some_and(|value| value.get("coverage_report").is_some()) {
        let report_arg = arguments
            .windows(2)
            .find(|pair| pair[0] == "--coverage-report")
            .map(|pair| pair[1].as_str())
            .unwrap_or(".open-contract/coverage/latest.json");
        let report_path = temporary
            .as_ref()
            .map_or_else(|| PathBuf::from(report_arg), |directory| directory.path().join(report_arg));
        let report_bytes = fs::read(&report_path)
            .map_err(|error| format!("cannot read CLI coverage report {}: {error}", report_path.display()))?;
        let report: Value = serde_json::from_slice(&report_bytes)
            .map_err(|error| format!("CLI coverage report is not JSON: {error}"))?;
        actual_value["coverage_report"] = report;
    }
    if let Some(expected) = expected {
        if expected["exit_code"].as_u64() != Some(actual.exit_code as u64)
            || expected["stdout"].as_str() != Some(actual.stdout.as_str())
            || expected["stderr"].as_str() != Some(actual.stderr.as_str())
            || expected
                .get("coverage_report")
                .is_some_and(|report| actual_value.get("coverage_report") != Some(report))
        {
            return Err(format!(
                "complete CLI result differed; expected {expected}, got {actual_value}"
            ));
        }
    }
    if let Some(predicate) = case.predicate.as_deref() {
        match_output(&actual_value, None, Some(predicate))
            .map_err(|error| format!("complete CLI result did not satisfy predicate: {error}"))?;
    }
    Ok(())
}
