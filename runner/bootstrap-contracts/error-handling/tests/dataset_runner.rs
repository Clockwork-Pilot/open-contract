#[path = "support/validator.rs"]
mod validator;

#[test]
fn dataset_entries_exercise_public_api() {
    let cases = validator::execute_dataset().expect("error-handling dataset cases should pass");
    assert!(cases > 0, "error-handling dataset must contain cases");
}
