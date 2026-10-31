use open_contracts_predicates::evaluate_predicate;
use serde_json::Value;

const DATASET: &str = include_str!("../../dataset.json");

fn main() {
    let dataset: Value = serde_json::from_str(DATASET).expect("predicate dataset JSON is valid");
    let cases = dataset["cases"].as_array().expect("dataset has cases");
    let mut passed = 0usize;
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap_or("unknown");
        let predicate = case["predicate"].as_str().expect("predicate is a string");
        let actual = evaluate_predicate(&case["json"], predicate);
        let expected = case["expected"].as_bool().unwrap_or(false);
        if actual == expected {
            passed += 1;
        } else {
            failures.push(format!("cases/{id}: expected {expected}, got {actual}"));
        }
    }
    for failure in &failures {
        eprintln!("FAIL {failure}");
    }
    println!("Rust predicate dataset: {passed}/{} dataset cases passed", cases.len());
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
