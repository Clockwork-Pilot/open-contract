use bootstrap_dataset_harness::{
    load_dataset, load_dataset_with_schema, run_dataset, run_dataset_with_schema, Dataset,
    DatasetCase,
};
use serde_json::Value;
use std::path::PathBuf;

#[cfg(coverage)]
#[path = "support/profile.rs"]
mod profile;

fn dataset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dataset.json")
}

fn dataset_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dataset.schema.json")
}

fn nested_dataset(case: &DatasetCase) -> Result<Dataset, String> {
    let input = &case.input;
    let extension = input["extension"].as_str().unwrap_or_default();
    let document = input["document"].as_str().unwrap_or_default();
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = if extension.is_empty() {
        directory.path().join("dataset")
    } else {
        directory.path().join(format!("dataset.{extension}"))
    };
    std::fs::write(&path, document).map_err(|error| error.to_string())?;
    load_dataset(path)
}

fn expect_error(case: &DatasetCase) -> Result<(), String> {
    let expected = case.input["error"]
        .as_str()
        .ok_or_else(|| "input.error must be a string".to_string())?;
    match nested_dataset(case) {
        Ok(_) => Err(format!("expected load error containing {expected:?}")),
        Err(error) if error.contains(expected) => Ok(()),
        Err(error) => Err(format!(
            "expected error containing {expected:?}, got {error}"
        )),
    }
}

fn nested_dataset_file(case: &DatasetCase) -> Result<(tempfile::TempDir, PathBuf), String> {
    let extension = case.input["extension"].as_str().unwrap_or("json");
    let document = case.input["document"]
        .as_str()
        .ok_or_else(|| "input.document must be a string".to_string())?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join(format!("nested.{extension}"));
    std::fs::write(&path, document).map_err(|error| error.to_string())?;
    Ok((directory, path))
}

fn schema_fixture(case: &DatasetCase) -> Result<(tempfile::TempDir, PathBuf, PathBuf), String> {
    let extension = case.input["extension"].as_str().unwrap_or("json");
    let document = case.input["document"]
        .as_str()
        .ok_or_else(|| "input.document must be a string".to_string())?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let dataset_path = if extension.is_empty() {
        directory.path().join("nested")
    } else {
        directory.path().join(format!("nested.{extension}"))
    };
    let schema_path = directory.path().join("dataset.schema.json");
    if case.input["write_dataset"].as_bool() != Some(false) {
        std::fs::write(&dataset_path, document).map_err(|error| error.to_string())?;
    }
    if let Some(schema) = case.input["schema_document"].as_str() {
        std::fs::write(&schema_path, schema).map_err(|error| error.to_string())?;
    }
    Ok((directory, dataset_path, schema_path))
}

fn schema_error(case: &DatasetCase, error: String) -> Result<(), String> {
    let expected = case.input["error"]
        .as_str()
        .ok_or_else(|| "input.error must be a string".to_string())?;
    if error.contains(expected) {
        Ok(())
    } else {
        Err(format!(
            "expected error containing {expected:?}, got {error}"
        ))
    }
}

fn validate_case(_: &Dataset, case: &DatasetCase) -> Result<(), String> {
    let operation = case.input["operation"]
        .as_str()
        .ok_or_else(|| "input.operation must be a string".to_string())?;
    match operation {
        "load" => {
            let dataset = nested_dataset(case)?;
            if dataset.version != 1 || dataset.cases.len() != 1 {
                return Err("loaded dataset did not preserve version and case".into());
            }
            Ok(())
        }
        "load-error" => expect_error(case),
        "missing-file" => {
            let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
            let error = load_dataset(directory.path().join("missing.json"))
                .expect_err("missing dataset should fail");
            if error.contains(case.input["error"].as_str().unwrap_or_default()) {
                Ok(())
            } else {
                Err(error)
            }
        }
        "round-trip" => {
            let dataset = nested_dataset(case)?;
            let serialized = serde_json::to_string(&dataset).map_err(|error| error.to_string())?;
            let case = &dataset.cases[0];
            if case.coverage_fingerprint.as_deref() != Some("00000000000000000000000000000000")
                || case.setup.is_some()
                || case.expected != Some(Value::Null)
                || serialized.contains("\"setup\"")
            {
                return Err(
                    "default fields or explicit null expected value were not preserved".into(),
                );
            }
            Ok(())
        }
        "missing-expected" => {
            let dataset = nested_dataset(case)?;
            if dataset.cases[0]
                .expected_value()
                .err()
                .is_some_and(|error| error == "expected is required for this case")
            {
                Ok(())
            } else {
                Err("expected_value did not reject a missing expected value".into())
            }
        }
        "run" => {
            let (_directory, path) = nested_dataset_file(case)?;
            let mut visited = Vec::new();
            let count = run_dataset(path, |_, nested_case| {
                visited.push(nested_case.id.clone());
                Ok(())
            })?;
            if count == 2 && visited == ["one", "two"] {
                Ok(())
            } else {
                Err(format!("unexpected nested dataset visits: {visited:?}"))
            }
        }
        "run-error" => {
            let (_directory, path) = nested_dataset_file(case)?;
            let expected = case.input["error"].as_str().unwrap_or_default();
            match run_dataset(path, |_, _| Ok(())) {
                Ok(_) => Err(format!(
                    "expected run_dataset error containing {expected:?}"
                )),
                Err(error) if error.contains(expected) => Ok(()),
                Err(error) => Err(format!("unexpected run_dataset error: {error}")),
            }
        }
        "run-validator-error" => {
            let (_directory, path) = nested_dataset_file(case)?;
            let mut visited = Vec::new();
            let error = run_dataset(path, |_, nested_case| {
                visited.push(nested_case.id.clone());
                if nested_case.id == "bad" {
                    Err("validator rejected this input".into())
                } else {
                    Ok(())
                }
            })
            .expect_err("validator failure should fail the run");
            if visited == ["bad", "good"] && error.contains("cases/bad") {
                Ok(())
            } else {
                Err(format!(
                    "unexpected callback failure behavior: {error}; {visited:?}"
                ))
            }
        }
        "schema-load" => {
            let (_directory, dataset_path, schema_path) = schema_fixture(case)?;
            let dataset = load_dataset_with_schema(&dataset_path, &schema_path)?;
            if dataset.version == 1 && dataset.cases.len() == 1 {
                Ok(())
            } else {
                Err("schema-validated dataset did not preserve its version and case".into())
            }
        }
        "schema-load-error" => {
            let (_directory, dataset_path, schema_path) = schema_fixture(case)?;
            match load_dataset_with_schema(&dataset_path, &schema_path) {
                Ok(_) => Err("expected schema loading to fail".into()),
                Err(error) => schema_error(case, error),
            }
        }
        "schema-run"
        | "schema-run-auto"
        | "schema-run-selected"
        | "schema-run-selected-no-schema" => {
            let (_directory, dataset_path, schema_path) = schema_fixture(case)?;
            let mut visited = Vec::new();
            let count = if matches!(
                operation,
                "schema-run-selected" | "schema-run-selected-no-schema"
            ) {
                std::env::set_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH", &dataset_path);
                if operation == "schema-run-selected-no-schema" {
                    std::env::set_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA", "");
                } else {
                    std::env::set_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA", &schema_path);
                }
                let ignored_path = _directory.path().join("ignored.json");
                let ignored_schema = _directory.path().join("ignored.schema.json");
                run_dataset_with_schema(&ignored_path, &ignored_schema, |_, nested_case| {
                    visited.push(nested_case.id.clone());
                    Ok(())
                })?
            } else if operation == "schema-run-auto" {
                run_dataset(&dataset_path, |_, nested_case| {
                    visited.push(nested_case.id.clone());
                    Ok(())
                })?
            } else {
                run_dataset_with_schema(&dataset_path, &schema_path, |_, nested_case| {
                    visited.push(nested_case.id.clone());
                    Ok(())
                })?
            };
            if count == 1 && visited == ["sample"] {
                Ok(())
            } else {
                Err(format!("unexpected schema-validated visits: {visited:?}"))
            }
        }
        "schema-run-error" => {
            let (_directory, dataset_path, schema_path) = schema_fixture(case)?;
            let mut called = false;
            match run_dataset_with_schema(&dataset_path, &schema_path, |_, _| {
                called = true;
                Ok(())
            }) {
                Ok(_) => Err("expected invalid schema input to fail before validation".into()),
                Err(error) if called => Err("schema failure invoked the case validator".into()),
                Err(error) => schema_error(case, error),
            }
        }
        "schema-run-duplicate" => {
            let (_directory, dataset_path, schema_path) = schema_fixture(case)?;
            let error = run_dataset_with_schema(&dataset_path, &schema_path, |_, _| Ok(()))
                .expect_err("duplicate case IDs should fail");
            schema_error(case, error)
        }
        other => Err(format!("unknown operation {other:?}")),
    }
}

fn execute_dataset() -> Result<usize, String> {
    let dataset_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(dataset_path);
    std::env::remove_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH");
    let selected_schema = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA");
    std::env::remove_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA");
    let schema_path = selected_schema
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(dataset_schema_path);
    let dataset = load_dataset_with_schema(&dataset_path, &schema_path)?;
    if dataset.version != 1 || dataset.cases.is_empty() {
        return Err("the dataset harness contract dataset is malformed".into());
    }
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::with_capacity(dataset.cases.len());
    let mut failures = Vec::new();
    for case in &dataset.cases {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        let result = validate_case(&dataset, case);
        #[cfg(coverage)]
        if capture_path.is_some() {
            match profile::snapshot() {
                Ok(bytes) => snapshots.push((case.id.clone(), bytes)),
                Err(error) => failures.push(format!("cases/{}: {error}", case.id)),
            }
        }
        if let Err(error) = result {
            failures.push(format!("cases/{}: {error}", case.id));
        }
    }
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)
            .map_err(|error| format!("cannot restore aggregate coverage counters: {error}"))?;
        profile::write_snapshots(PathBuf::from(path).as_path(), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    if failures.is_empty() {
        Ok(dataset.cases.len())
    } else {
        Err(failures.join("; "))
    }
}

#[test]
fn dataset_entries_exercise_public_api() {
    let count = execute_dataset().expect("bootstrap dataset harness cases should pass");
    assert!(count > 0, "bootstrap dataset must contain cases");
}
