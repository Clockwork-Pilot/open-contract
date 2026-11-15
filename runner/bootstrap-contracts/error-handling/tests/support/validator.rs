use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_errors::ContractError;
use serde_json::json;
use std::path::Path;

pub(crate) fn execute_dataset() -> Result<usize, String> {
    let package = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let result = run_dataset(package.join("dataset.json"), |_, case| {
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
    let input = &case.input;
    let code = input["exit_code"]
        .as_u64()
        .and_then(|code| u8::try_from(code).ok())
        .ok_or_else(|| "exit_code must fit in a byte".to_string())?;
    let message = input["message"]
        .as_str()
        .ok_or_else(|| "message must be a string".to_string())?;
    let mut error = if input["from_tuple"].as_bool() == Some(true) {
        (code, message.to_owned()).into()
    } else {
        ContractError::new(code, message)
    };
    if let Some(contexts) = input["contexts"].as_array() {
        for context in contexts {
            let layer = context
                .as_str()
                .ok_or_else(|| "contexts must contain strings".to_string())?;
            error = error.context(layer);
        }
    }
    let actual = json!({
        "exit_code": error.exit_code(),
        "message": error.message(),
        "rendered": error.render(),
        "display": error.to_string(),
    });
    let expected = case.expected_value()?;
    if &actual != expected {
        return Err(format!("expected {expected}, got {actual}"));
    }
    Ok(())
}
