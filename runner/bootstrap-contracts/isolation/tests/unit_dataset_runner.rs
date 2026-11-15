#[path = "support/validator.rs"]
mod validator;

#[test]
fn dataset_entries_exercise_public_isolation_api() {
    let cases = validator::execute_dataset().expect("isolation dataset cases should pass");
    assert!(cases > 0, "isolation dataset must contain cases");
}
