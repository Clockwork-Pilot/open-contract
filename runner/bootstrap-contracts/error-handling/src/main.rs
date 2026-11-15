use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_errors::ContractError;
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error-handling dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let dataset_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dataset.json");
    let cases = run_dataset(dataset_path, |_, case| validate_case(case))?;
    println!("Error-handling dataset: {cases} cases passed");
    Ok(())
}

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
