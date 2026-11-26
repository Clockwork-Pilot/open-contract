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
    run_dataset_inner(path.as_ref(), &mut validate)
}

fn run_dataset_inner(
    path: &Path,
    validate: &mut dyn FnMut(&Dataset, &DatasetCase) -> Result<(), String>,
) -> Result<usize, String> {
    let dataset = load_dataset(path)?;
    if dataset.version != 1 {
        return Err(format!(
            "unsupported bootstrap dataset version {}; expected 1",
            dataset.version
        ));
    }
    if dataset.cases.is_empty() {
        return Err("dataset must contain at least one case".into());
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
