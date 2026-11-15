#[path = "support/validator.rs"]
mod validator;

#[test]
fn dataset_entries_exercise_public_api() {
    let cases = validator::execute_dataset().expect("dataset-generation cases should pass");
    assert!(cases > 0, "dataset-generation dataset must contain cases");
}
