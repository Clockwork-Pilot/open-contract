use open_contracts_predicates::evaluate_predicate;
use serde_json::Value;
use std::process::ExitCode;

const DATASET: &str = include_str!("../dataset.json");

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
    let dataset: Value = serde_json::from_str(DATASET)
        .map_err(|error| format!("invalid predicate dataset JSON: {error}"))?;
    let cases = dataset["cases"]
        .as_array()
        .ok_or_else(|| "predicate dataset must contain a cases array".to_string())?;
    let mut passed = 0usize;
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap_or("unknown");
        let predicate = case["predicate"]
            .as_str()
            .ok_or_else(|| format!("cases/{id}: predicate must be a string"))?;
        let expected = case["expected"]
            .as_bool()
            .ok_or_else(|| format!("cases/{id}: expected must be boolean"))?;
        let actual = evaluate_predicate(&case["json"], predicate);
        if actual == expected {
            passed += 1;
        } else {
            failures.push(format!("cases/{id}: expected {expected}, got {actual}"));
        }
        let _ = index;
    }
    println!(
        "Rust predicate dataset: {passed}/{} dataset cases passed",
        cases.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}
