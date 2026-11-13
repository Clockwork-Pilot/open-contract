use open_contract_verification::{match_output, MatchKind};
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
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
    let bytes = std::fs::read(package.join("dataset.json"))
        .map_err(|error| format!("cannot read dataset: {error}"))?;
    let dataset: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid dataset JSON: {error}"))?;
    let cases = dataset["cases"]
        .as_array()
        .ok_or_else(|| "dataset must contain a cases array".to_string())?;
    if cases.is_empty() {
        return Err("dataset must exercise output verification".into());
    }
    for case in cases {
        let id = case["id"].as_str().unwrap_or("unknown");
        check(id, &case["input"]["verify"])?;
        let alternatives = case["input"]["verify_also"]
            .as_array()
            .ok_or_else(|| format!("cases/{id}: verify_also must be an array"))?;
        for additional in alternatives {
            check(id, additional)?;
        }
    }
    println!("Verification dataset: {} cases passed", cases.len());
    Ok(())
}

fn check(id: &str, check: &Value) -> Result<(), String> {
    let actual = &check["actual"];
    let expected = check.get("expected");
    let predicate = check.get("predicate").and_then(Value::as_str);
    let expected_valid = check["valid"]
        .as_bool()
        .ok_or_else(|| format!("cases/{id}: valid must be boolean"))?;
    let result = match_output(actual, expected, predicate);
    if result.is_ok() != expected_valid {
        return Err(format!(
            "cases/{id}: validity mismatch; expected {expected_valid}"
        ));
    }
    if let Ok(matched) = result {
        match (expected, predicate, matched) {
            (Some(_), None, MatchKind::Exact) | (None, Some(_), MatchKind::Predicate) => {}
            (Some(_), Some(_), MatchKind::ExpectedAndPredicate) => {}
            _ => {
                return Err(format!(
                    "cases/{id}: verifier returned an unexpected match kind"
                ))
            }
        }
    }
    Ok(())
}
