#[path = "support/validator.rs"]
mod validator;

use runner_batch::{load_dataset, load_dataset_with_schema};
use std::fs;

#[test]
fn dataset_entries_exercise_public_api() {
    let cases = validator::execute_dataset().expect("batch execution dataset cases should pass");
    assert!(cases > 0, "batch execution dataset must contain cases");
}

#[test]
fn per_dataset_yaml_schemas_validate_calls_and_sequence_steps() {
    let directory = tempfile::tempdir().unwrap();
    let first_dataset = directory.path().join("first.yaml");
    let second_dataset = directory.path().join("second.yaml");
    let first_schema = directory.path().join("first-schema.yaml");
    let second_schema = directory.path().join("second-schema.yaml");
    fs::write(
        &first_dataset,
        "version: 1\ncases:\n  - id: direct\n    input: {name: Ada}\n    expected: {ok: true}\n  - id: sequence\n    steps:\n      - id: one\n        input: {name: Grace}\n        expected: {ok: false}\n      - id: two\n        input: {name: Lin}\n        expected: {ok: true}\n",
    )
    .unwrap();
    fs::write(
        &second_dataset,
        "version: 1\ncases:\n  - id: other-shape\n    input: 7\n    expected: done\n",
    )
    .unwrap();
    fs::write(
        &first_schema,
        "input:\n  type: object\n  required: [name]\n  fields:\n    name: string\nexpected:\n  type: object\n  fields:\n    ok: boolean\n",
    )
    .unwrap();
    fs::write(&second_schema, "input: integer\nexpected: string\n").unwrap();

    load_dataset_with_schema(&first_dataset, Some(&first_schema))
        .expect("the first dataset uses its own object schemas");
    load_dataset_with_schema(&second_dataset, Some(&second_schema))
        .expect("the second dataset may use different value shapes");

    fs::write(
        &first_dataset,
        "version: 1\ncases:\n  - id: bad-step\n    steps:\n      - id: invalid\n        input: {name: 42}\n        expected: {ok: true}\n",
    )
    .unwrap();
    let error = load_dataset_with_schema(&first_dataset, Some(&first_schema))
        .expect_err("invalid step input must be rejected before execution");
    assert!(error.contains("bad-step step invalid"));
    assert!(error.contains("input at $.name: expected string"));
}

#[test]
fn runner_dataset_schema_accepts_boolean_parallel_steps_and_rejects_group_form() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dataset.yaml");
    fs::write(
        &path,
        "version: 1\ncases:\n  - id: checks\n    steps:\n      - id: first\n        input: {value: 1}\n        expected: {accepted: true}\n        parallel: true\n      - id: second\n        input: {value: 2}\n        expected: {accepted: true}\n        parallel: true\n      - id: third\n        input: {value: 3}\n        expected: {accepted: true}\n",
    )
    .unwrap();
    let dataset = load_dataset(&path).expect("boolean parallel steps match the shared schema");
    let runner_batch::CaseSpec::Sequence(sequence) = &dataset.cases[0] else {
        panic!("expected a sequence case");
    };
    assert!(sequence.steps[0].parallel);
    assert!(sequence.steps[1].parallel);
    assert!(!sequence.steps[2].parallel);

    fs::write(
        &path,
        "version: 1\ncases:\n  - id: checks\n    steps:\n      - parallel:\n          - id: first\n            input: {value: 1}\n            expected: {accepted: true}\n",
    )
    .unwrap();
    assert!(
        load_dataset(&path).is_err(),
        "group form is outside the schema"
    );
}
