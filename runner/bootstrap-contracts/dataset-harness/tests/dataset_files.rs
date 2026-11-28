use bootstrap_dataset_harness::{load_dataset, load_dataset_with_schema, run_dataset};
use serde_json::json;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn package_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn loads_json_dataset_and_preserves_public_case_values() {
    let dataset = load_dataset(fixture("valid.json")).expect("load JSON dataset");
    assert_eq!(dataset.version, 1);
    assert_eq!(dataset.cases.len(), 1);
    assert_eq!(dataset.cases[0].id, "json-case");
    assert_eq!(dataset.cases[0].setup.as_deref(), Some("json-case"));
    assert_eq!(dataset.cases[0].input, json!({"value": 7}));
    assert_eq!(dataset.cases[0].expected, Some(json!({"value": 7})));
}

#[test]
fn loads_yaml_dataset_with_result_predicate() {
    let dataset = load_dataset(fixture("valid.yaml")).expect("load YAML dataset");
    assert_eq!(dataset.version, 1);
    assert_eq!(dataset.cases.len(), 1);
    assert_eq!(dataset.cases[0].id, "yaml-case");
    assert_eq!(dataset.cases[0].setup, None);
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
        assert_eq!(
            case.coverage_fingerprint.as_deref(),
            Some("00000000000000000000000000000000")
        );
        Ok(())
    })
    .expect("missing fingerprint must not prevent dataset execution");
    assert!(called);
}

#[test]
fn package_dataset_and_case_values_match_its_schema() {
    let dataset = load_dataset_with_schema(
        &package_file("dataset.json"),
        &package_file("dataset.schema.json"),
    )
    .expect("package dataset should match its schema");
    assert_eq!(dataset.version, 1);
    assert_eq!(dataset.cases.len(), 32);
}

#[test]
fn schema_reports_invalid_case_input_and_expected_paths() {
    let directory = tempfile::tempdir().unwrap();
    let dataset = directory.path().join("dataset.json");
    std::fs::write(
        &dataset,
        r#"{"version":1,"cases":[{"id":"bad-input","input":{"operation":"invented"},"expected":{"ok":true}}]}"#,
    )
    .unwrap();
    let input_error = load_dataset_with_schema(&dataset, &package_file("dataset.schema.json"))
        .expect_err("unknown operation should violate input schema");
    assert!(input_error.contains("/cases/0/input"), "{input_error}");

    std::fs::write(
        &dataset,
        r#"{"version":1,"cases":[{"id":"bad-expected","input":{"operation":"missing-file","error":"cannot read dataset"},"expected":{"ok":false}}]}"#,
    )
    .unwrap();
    let expected_error = load_dataset_with_schema(&dataset, &package_file("dataset.schema.json"))
        .expect_err("unexpected result should violate expected schema");
    assert!(
        expected_error.contains("/cases/0/expected"),
        "{expected_error}"
    );

    std::fs::write(
        &dataset,
        r#"{"version":1,"cases":[{"id":"unknown-field","input":{"operation":"missing-file","error":"cannot read dataset"},"expected":{"ok":true},"unclassified":true}]}"#,
    )
    .unwrap();
    let unknown_field_error =
        load_dataset_with_schema(&dataset, &package_file("dataset.schema.json"))
            .expect_err("unknown case fields should violate the schema");
    assert!(
        unknown_field_error.contains("/cases/0"),
        "{unknown_field_error}"
    );
}

#[test]
fn run_dataset_uses_a_neighboring_dataset_schema() {
    let directory = tempfile::tempdir().unwrap();
    let dataset = directory.path().join("dataset.json");
    let schema = directory.path().join("dataset.schema.json");
    std::fs::write(
        &schema,
        r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","required":["version","cases"],"properties":{"version":{"const":1},"cases":{"type":"array","items":{"type":"object","required":["id","input","expected"],"properties":{"id":{"type":"string"},"input":{"type":"object","required":["value"],"properties":{"value":{"type":"integer"}},"additionalProperties":false},"expected":{}}}}}}"#,
    )
    .unwrap();
    std::fs::write(
        &dataset,
        r#"{"version":1,"cases":[{"id":"missing-value","input":{},"expected":true}]}"#,
    )
    .unwrap();
    let error = run_dataset(&dataset, |_, _| Ok(()))
        .expect_err("neighboring schema should be enforced before validation");
    assert!(error.contains("/cases/0/input"), "{error}");
}

#[test]
fn duplicate_case_ids_are_rejected_before_validation() {
    let directory = tempfile::tempdir().unwrap();
    let dataset = directory.path().join("dataset.json");
    std::fs::write(
        &dataset,
        r#"{"version":1,"cases":[{"id":"same","input":{},"expected":true},{"id":"same","input":{},"expected":true}]}"#,
    )
    .unwrap();
    let mut called = false;
    let error = run_dataset(&dataset, |_, _| {
        called = true;
        Ok(())
    })
    .expect_err("duplicate case IDs should be rejected");
    assert!(error.contains("duplicate case ID"), "{error}");
    assert!(!called, "invalid dataset should not invoke the validator");
}
