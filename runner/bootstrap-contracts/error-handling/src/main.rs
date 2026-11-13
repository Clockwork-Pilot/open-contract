use open_contract_errors::ContractError;
use serde_json::{json, Value};
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
    let bytes =
        std::fs::read(dataset_path).map_err(|error| format!("cannot read dataset: {error}"))?;
    let dataset: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid dataset JSON: {error}"))?;
    let cases = dataset["cases"]
        .as_array()
        .ok_or_else(|| "dataset must contain a cases array".to_string())?;
    if cases.is_empty() {
        return Err("dataset must exercise error reporting".into());
    }

    for case in cases {
        let id = case["id"].as_str().unwrap_or("unknown");
        let input = &case["input"];
        let code = input["exit_code"]
            .as_u64()
            .and_then(|code| u8::try_from(code).ok())
            .ok_or_else(|| format!("cases/{id}: exit_code must fit in a byte"))?;
        let message = input["message"]
            .as_str()
            .ok_or_else(|| format!("cases/{id}: message must be a string"))?;
        let mut error = if input["from_tuple"].as_bool() == Some(true) {
            (code, message.to_owned()).into()
        } else {
            ContractError::new(code, message)
        };
        if let Some(contexts) = input["contexts"].as_array() {
            for context in contexts {
                let layer = context
                    .as_str()
                    .ok_or_else(|| format!("cases/{id}: contexts must contain strings"))?;
                error = error.context(layer);
            }
        }
        let actual = json!({
            "exit_code": error.exit_code(),
            "message": error.message(),
            "rendered": error.render(),
            "display": error.to_string(),
        });
        if actual != case["expected"] {
            return Err(format!(
                "cases/{id}: expected {}, got {actual}",
                case["expected"]
            ));
        }
    }

    println!("Error-handling dataset: {} cases passed", cases.len());
    Ok(())
}
