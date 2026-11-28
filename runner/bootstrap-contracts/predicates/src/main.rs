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
    let manifest_path = package
        .parent()
        .ok_or_else(|| "predicate package has no bootstrap-contracts parent".to_string())?
        .join("datasets.manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path)
            .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    let contract = manifest["contracts"]
        .as_array()
        .and_then(|contracts| contracts.iter().find(|contract| contract["package"] == "predicates"))
        .ok_or_else(|| "predicate package is not declared in datasets.manifest.json".to_string())?;
    let datasets = contract["datasets"]
        .as_array()
        .ok_or_else(|| "predicate contract datasets must be an array".to_string())?;
    let mut total = 0;
    for dataset in datasets {
        let id = dataset["id"]
            .as_str()
            .ok_or_else(|| "predicate dataset ID must be a string".to_string())?;
        let path = dataset["path"]
            .as_str()
            .ok_or_else(|| format!("predicate dataset {id} path must be a string"))?;
        let selected_path = package.join(path);
        std::env::set_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH", &selected_path);
        if let Some(schema) = dataset["schema"].as_str() {
            std::env::set_var(
                "OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA",
                package.join(schema),
            );
        } else {
            std::env::remove_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA");
        }
        let cases = run_dataset(selected_path, |_, case| validate_case(case))?;
        println!("Rust predicate dataset {id}: {cases}/{cases} cases passed");
        total += cases;
    }
    if total == 0 {
        return Err("predicate contract must declare at least one dataset case".into());
    }
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
