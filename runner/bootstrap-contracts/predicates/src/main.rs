use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_predicates::evaluate_predicate;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("predicate dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases = run_dataset(package.join("dataset.json"), |_, case| validate_case(case))?;
    println!("Rust predicate dataset: {cases}/{cases} dataset cases passed");
    Ok(())
}

fn validate_case(case: &DatasetCase) -> Result<(), String> {
    let predicate = case.input["predicate"]
        .as_str()
        .ok_or_else(|| "input.predicate must be a string".to_string())?;
    let expected = case
        .expected_value()?
        .as_bool()
        .ok_or_else(|| "expected must be boolean".to_string())?;
    let actual = evaluate_predicate(&case.input["json"], predicate);
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}, got {actual}"))
    }
}
