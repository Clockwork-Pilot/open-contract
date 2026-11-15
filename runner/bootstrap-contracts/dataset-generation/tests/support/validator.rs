use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_dataset_generation::generate_dataset;
use std::path::Path;

pub(crate) fn execute_dataset() -> Result<usize, String> {
    let package = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let result = run_dataset(package.join("dataset.yaml"), |_, case| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        validate_case(case)?;
        #[cfg(coverage)]
        if capture_path.is_some() {
            snapshots.push((case.id.clone(), profile::snapshot()?));
        }
        Ok(())
    });
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)?;
        profile::write_snapshots(Path::new(&path), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    result
}

#[cfg(coverage)]
#[path = "profile.rs"]
mod profile;

fn validate_case(case: &DatasetCase) -> Result<(), String> {
    let schema = case
        .input
        .get("schema")
        .cloned()
        .ok_or_else(|| "case input must provide a schema".to_string())?;
    let expected = case.expected_value()?;
    let actual = match generate_dataset(&schema) {
        Ok(actual) => actual,
        Err(error) => {
            if expected["error_contains"]
                .as_str()
                .is_some_and(|needle| error.contains(needle))
            {
                return Ok(());
            }
            return Err(error);
        }
    };
    if let Some(needle) = expected["error_contains"].as_str() {
        return Err(format!(
            "expected generation error containing {needle:?}, got output"
        ));
    }
    if actual != *expected {
        return Err(format!(
            "generated dataset differs from expected output; actual: {}",
            serde_json::to_string(&actual).unwrap_or_else(|_| "<unavailable>".into())
        ));
    }
    Ok(())
}
