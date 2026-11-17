#[path = "support/validator.rs"]
mod validator;

use runner_batch::load_dataset;
use std::fs;

#[test]
fn dataset_entries_exercise_public_api() {
    let cases = validator::execute_dataset().expect("batch execution dataset cases should pass");
    assert!(cases > 0, "batch execution dataset must contain cases");
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
