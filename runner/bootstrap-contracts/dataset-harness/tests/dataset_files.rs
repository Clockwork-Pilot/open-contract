use bootstrap_dataset_harness::{load_dataset, run_dataset};
use serde_json::json;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn loads_json_dataset_and_preserves_public_case_values() {
    let dataset = load_dataset(fixture("valid.json")).expect("load JSON dataset");
    assert_eq!(dataset.version, 1);
    assert_eq!(dataset.cases.len(), 1);
    assert_eq!(dataset.cases[0].id, "json-case");
    assert!(dataset.cases[0].setup);
    assert_eq!(dataset.cases[0].input, json!({"value": 7}));
    assert_eq!(dataset.cases[0].expected, Some(json!({"value": 7})));
}

#[test]
fn loads_yaml_dataset_with_result_predicate() {
    let dataset = load_dataset(fixture("valid.yaml")).expect("load YAML dataset");
    assert_eq!(dataset.version, 1);
    assert_eq!(dataset.cases.len(), 1);
    assert_eq!(dataset.cases[0].id, "yaml-case");
    assert!(!dataset.cases[0].setup);
    assert_eq!(dataset.cases[0].input, json!({"value": 9}));
    assert_eq!(dataset.cases[0].predicate.as_deref(), Some(".value == 9"));
}

#[test]
fn run_dataset_checks_envelope_before_visiting_entries() {
    let dataset = run_dataset(fixture("valid.yaml"), |_, case| {
        assert_eq!(case.id, "yaml-case");
        Ok(())
    })
    .expect("valid envelope should execute");
    assert_eq!(dataset, 1);
}

#[test]
fn missing_fingerprint_becomes_zero_sentinel_without_skipping_case() {
    let mut called = false;
    run_dataset(fixture("missing-fingerprint.json"), |_, case| {
        called = true;
        assert_eq!(case.coverage_fingerprint.as_deref(), Some("00000000000000000000000000000000"));
        Ok(())
    })
    .expect("missing fingerprint must not prevent dataset execution");
    assert!(called);
}
