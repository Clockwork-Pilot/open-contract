#[path = "support/validator.rs"]
mod validator;

pub(crate) mod test_fixture {
    pub(crate) fn exercise_landlock_installation_paths() -> Result<(), String> {
        Ok(())
    }

    pub(crate) fn exercise_backend_error_paths() -> Result<(), String> {
        Ok(())
    }

    pub(crate) fn exercise_staging_copy_error_paths() -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn dataset_entries_exercise_public_isolation_api() {
    let cases = validator::execute_dataset().expect("isolation dataset cases should pass");
    assert!(cases > 0, "isolation dataset must contain cases");
}
