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
    let expected_valid = case.expected_value()?["valid"]
        .as_bool()
        .ok_or_else(|| "expected.valid must be boolean".to_string())?;
    let result = match_output(actual, expected, predicate);
    if result.is_ok() != expected_valid {
        return Err(format!("validity mismatch; expected {expected_valid}"));
    }
    if let Ok(matched) = result {
        match (expected, predicate, matched) {
            (Some(_), None, MatchKind::Exact) | (None, Some(_), MatchKind::Predicate) => {}
            (Some(_), Some(_), MatchKind::ExpectedAndPredicate) => {}
            _ => return Err("verifier returned an unexpected match kind".into()),
        }
    }
    Ok(())
}
