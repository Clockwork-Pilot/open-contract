use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub version: u32,
    pub cases: Vec<DatasetCase>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetCase {
    pub id: String,
    pub input: Value,
    #[serde(
        default = "missing_coverage_fingerprint",
        skip_serializing_if = "Option::is_none"
    )]
    pub coverage_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_expected",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
}

fn missing_coverage_fingerprint() -> Option<String> {
    Some("0".repeat(32))
}

fn deserialize_expected<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

impl DatasetCase {
    pub fn expected_value(&self) -> Result<&Value, String> {
        self.expected
            .as_ref()
            .ok_or_else(|| "expected is required for this case".into())
    }
}

/// Load the shared bootstrap dataset envelope from JSON or YAML.
pub fn load_dataset(path: impl AsRef<Path>) -> Result<Dataset, String> {
    load_dataset_path(path.as_ref())
}

/// Load a JSON or YAML dataset and validate its complete document against a
/// package-owned JSON Schema before deserializing the shared envelope.
pub fn load_dataset_with_schema(
    dataset_path: &Path,
    schema_path: &Path,
) -> Result<Dataset, String> {
    let value = load_dataset_value(dataset_path)?;
    validate_dataset_schema(&value, dataset_path, schema_path)?;
    serde_json::from_value(value).map_err(|error| {
        format!(
            "invalid dataset {} after schema validation: {error}",
            dataset_path.display()
        )
    })
}

fn load_dataset_value(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read dataset: {error}"))?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => {
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid dataset JSON: {error}"))
        }
        Some("yaml" | "yml") => {
            serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid dataset YAML: {error}"))
        }
        Some(extension) => Err(format!("unsupported dataset extension: {extension}")),
        None => Err("dataset path must have a .json, .yaml, or .yml extension".into()),
    }
}

fn validate_dataset_schema(
    value: &Value,
    dataset_path: &Path,
    schema_path: &Path,
) -> Result<(), String> {
    let schema_bytes = fs::read(schema_path).map_err(|error| {
        format!(
            "cannot read dataset schema {}: {error}",
            schema_path.display()
        )
    })?;
    let schema: Value = serde_json::from_slice(&schema_bytes)
        .map_err(|error| format!("invalid dataset schema {}: {error}", schema_path.display()))?;
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        format!(
            "cannot compile dataset schema {}: {error}",
            schema_path.display()
        )
    })?;
    validator.validate(value).map_err(|error| {
        format!(
            "dataset {} does not match schema {} at {}: {error}",
            dataset_path.display(),
            schema_path.display(),
            error.instance_path()
        )
    })
}

fn load_dataset_path(path: &Path) -> Result<Dataset, String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read dataset: {error}"))?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => {
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid dataset JSON: {error}"))
        }
        Some("yaml" | "yml") => {
            serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid dataset YAML: {error}"))
        }
        Some(extension) => Err(format!("unsupported dataset extension: {extension}")),
        None => Err("dataset path must have a .json, .yaml, or .yml extension".into()),
    }
}

/// Validate the shared envelope and call the contract's validator once for
/// each entry, adding the case ID to any validation failure.
pub fn run_dataset(
    path: impl AsRef<Path>,
    mut validate: impl FnMut(&Dataset, &DatasetCase) -> Result<(), String>,
) -> Result<usize, String> {
    let (path, schema_path) = select_dataset_paths(path.as_ref(), None);
    run_dataset_inner(&path, schema_path.as_deref(), &mut validate)
}

/// Run a dataset after validating its envelope, inputs, and expected values
/// against an explicit package-owned JSON Schema.
pub fn run_dataset_with_schema(
    path: impl AsRef<Path>,
    schema_path: impl AsRef<Path>,
    mut validate: impl FnMut(&Dataset, &DatasetCase) -> Result<(), String>,
) -> Result<usize, String> {
    let (path, schema_path) = select_dataset_paths(path.as_ref(), Some(schema_path.as_ref()));
    run_dataset_inner(&path, schema_path.as_deref(), &mut validate)
}

fn select_dataset_paths(
    default_path: &Path,
    default_schema: Option<&Path>,
) -> (std::path::PathBuf, Option<std::path::PathBuf>) {
    let selected_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH");
    std::env::remove_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_PATH");
    let path = selected_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| default_path.to_path_buf());

    let selected_schema = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA");
    std::env::remove_var("OPEN_CONTRACT_BOOTSTRAP_DATASET_SCHEMA");
    let schema_path = match selected_schema {
        Some(schema) => {
            if schema.is_empty() {
                return (path, None);
            }
            Some(std::path::PathBuf::from(schema))
        }
        None => default_schema
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                let adjacent = path.with_file_name("dataset.schema.json");
                adjacent.is_file().then_some(adjacent)
            }),
    };
    (path, schema_path)
}

fn run_dataset_inner(
    path: &Path,
    schema_path: Option<&Path>,
    validate: &mut dyn FnMut(&Dataset, &DatasetCase) -> Result<(), String>,
) -> Result<usize, String> {
    let dataset = match schema_path {
        Some(schema_path) => load_dataset_with_schema(path, schema_path)?,
        None => load_dataset(path)?,
    };
    if dataset.version != 1 {
        return Err(format!(
            "unsupported bootstrap dataset version {}; expected 1",
            dataset.version
        ));
    }
    if dataset.cases.is_empty() {
        return Err("dataset must contain at least one case".into());
    }
    let mut case_ids = std::collections::BTreeSet::new();
    for case in &dataset.cases {
        if !case_ids.insert(case.id.as_str()) {
            return Err(format!(
                "duplicate case ID {:?} in {}",
                case.id,
                path.display()
            ));
        }
    }
    let mut failures = Vec::new();
    for case in &dataset.cases {
        if case.expected.is_none() && case.predicate.is_none() {
            failures.push(format!(
                "cases/{}: expected or predicate is required",
                case.id
            ));
            continue;
        }
        let outcome = validate(&dataset, case);
        if let Err(error) = outcome {
            failures.push(format!("cases/{}: {error}", case.id));
        }
    }
    if failures.is_empty() {
        Ok(dataset.cases.len())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(test)]
extern crate self as bootstrap_dataset_harness;

#[cfg(test)]
#[path = "../tests/unit_dataset_runner.rs"]
mod dataset_runner_test;
