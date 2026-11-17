use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_verification::{match_output, MatchKind};
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

pub fn entrypoint() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("verification dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let cases = run_dataset(package.join("dataset.json"), |_, case| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        let result = validate_case(case);
        #[cfg(coverage)]
        if capture_path.is_some() {
            let bytes = profile::snapshot()?;
            snapshots.push((case.id.clone(), bytes));
        }
        result
    })?;
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)?;
        profile::write_snapshots(PathBuf::from(path).as_path(), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    println!("Verification dataset: {cases} cases passed");
    Ok(())
}

#[cfg(coverage)]
#[path = "profile.rs"]
mod profile;

fn validate_case(case: &DatasetCase) -> Result<(), String> {
    let input = &case.input;
    let actual = &input["actual"];
    let expected = input.get("expected");
    let predicate = input.get("predicate").and_then(Value::as_str);
    let result = match_output(actual, expected, predicate);
    let actual_result = match result {
        Ok(MatchKind::Expected) => serde_json::json!("expected"),
        Ok(MatchKind::ExpectedAndPredicate) => serde_json::json!("expected_and_predicate"),
        Ok(MatchKind::Predicate) => serde_json::json!("predicate"),
        Err(error) => serde_json::json!({ "error": error }),
    };
    let expected_result = case.expected_value()?;
    if !matches_result(&actual_result, expected_result) {
        return Err(format!("result mismatch; actual: {actual_result}"));
    }
    Ok(())
}

fn matches_result(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual_value| matches_result(actual_value, value))
        }),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(a, e)| matches_result(a, e))
        }
        _ => actual == expected,
    }
}
