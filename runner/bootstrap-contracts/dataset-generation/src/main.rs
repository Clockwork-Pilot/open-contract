use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_dataset_generation::generate_dataset;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dataset-generation dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases = run_dataset(package.join("dataset.yaml"), |_, case| validate_case(case))?;
    println!("Dataset-generation dataset: {cases} cases passed");
    Ok(())
}

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
            if let Some(needle) = expected["error_contains"].as_str() {
                if error.contains(needle) {
                    return Ok(());
                }
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
