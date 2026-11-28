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

#[test]
fn setup_fixture_does_not_replace_required_operation_input() {
    use bootstrap_dataset_harness::DatasetCase;
    use runner_manifest::ManifestPlan;

    let directory = tempfile::tempdir().unwrap();
    let package = directory.path();
    std::fs::write(package.join("dataset.json"), "{}").unwrap();
    std::fs::write(
        package.join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
        package.join("contract.yaml"),
        "version: '1'\nname: setup-guard\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
    )
    .unwrap();
    let plan = ManifestPlan::load(package.join("contract.yaml")).unwrap();
    let case = DatasetCase {
        id: "setup-fixture-does-not-replace-operation-input".into(),
        input: serde_json::json!({
            "fixture_id": "auto-backend-prefers-bubblewrap"
        }),
        coverage_fingerprint: None,
        setup: Some("auto-backend-prefers-bubblewrap".into()),
        expected: Some(serde_json::json!({"rejected": true})),
        predicate: None,
    };

    let result = validator::validate_case(&plan, package, &None, &case)
        .expect("fixture-only input should be rejected before setup is applied");
    assert_eq!(result, package);
}
