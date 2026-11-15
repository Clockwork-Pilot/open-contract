#[path = "support/validator.rs"]
mod validator;

#[test]
fn dataset_entries_exercise_public_api() {
    validator::run().expect("manifest dataset cases should pass");
}
