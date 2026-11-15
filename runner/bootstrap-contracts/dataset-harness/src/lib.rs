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
    #[serde(default, skip_serializing_if = "is_false")]
    pub setup: bool,
    #[serde(
        default,
        deserialize_with = "deserialize_expected",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
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
    let path = path.as_ref();
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
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::with_capacity(dataset.cases.len());
    let mut failures = Vec::new();
    for case in &dataset.cases {
        if case.expected.is_none() && case.predicate.is_none() {
            failures.push(format!(
                "cases/{}: expected or predicate is required",
                case.id
            ));
            continue;
        }
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        let outcome = validate(&dataset, case);
        #[cfg(coverage)]
        if capture_path.is_some() {
            match profile::snapshot() {
                Ok(bytes) => snapshots.push((case.id.clone(), bytes)),
                Err(error) => failures.push(format!("cases/{}: {error}", case.id)),
            }
        }
        if let Err(error) = outcome {
            failures.push(format!("cases/{}: {error}", case.id));
        }
    }
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)
            .map_err(|error| format!("cannot restore aggregate coverage counters: {error}"))?;
        profile::write_snapshots(Path::new(&path), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    #[cfg(not(coverage))]
    if capture_path.is_some() {
        return Err("coverage snapshots require an LLVM coverage-instrumented build".into());
    }
    if failures.is_empty() {
        Ok(dataset.cases.len())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(coverage)]
mod profile {
    use std::fs;
    use std::io;
    use std::path::Path;

    unsafe extern "C" {
        fn __llvm_profile_get_size_for_buffer() -> u64;
        fn __llvm_profile_write_buffer(buffer: *mut i8) -> i32;
        fn __llvm_profile_reset_counters();
        fn __llvm_profile_merge_from_buffer(buffer: *const i8, size: u64) -> i32;
    }

    const MAGIC: &[u8; 8] = b"OCOVCASE";

    pub(super) unsafe fn reset_counters() {
        __llvm_profile_reset_counters();
    }

    pub(super) fn snapshot() -> Result<Vec<u8>, String> {
        let size = unsafe { __llvm_profile_get_size_for_buffer() } as usize;
        let mut bytes = vec![0i8; size];
        let result = unsafe { __llvm_profile_write_buffer(bytes.as_mut_ptr()) };
        if result != 0 {
            return Err(format!("LLVM profile snapshot failed with status {result}"));
        }
        Ok(bytes.into_iter().map(|byte| byte as u8).collect())
    }

    pub(super) fn restore_aggregate(snapshots: &[(String, Vec<u8>)]) -> Result<(), String> {
        unsafe { __llvm_profile_reset_counters() };
        for (id, bytes) in snapshots {
            let result = unsafe {
                __llvm_profile_merge_from_buffer(bytes.as_ptr().cast(), bytes.len() as u64)
            };
            if result != 0 {
                return Err(format!(
                    "LLVM profile merge failed for case {id:?} with status {result}"
                ));
            }
        }
        Ok(())
    }

    pub(super) fn write_snapshots(path: &Path, snapshots: &[(String, Vec<u8>)]) -> io::Result<()> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&(snapshots.len() as u32).to_le_bytes());
        for (id, profile) in snapshots {
            let id = id.as_bytes();
            output.extend_from_slice(&(id.len() as u32).to_le_bytes());
            output.extend_from_slice(&(profile.len() as u64).to_le_bytes());
            output.extend_from_slice(id);
            output.extend_from_slice(profile);
        }
        fs::write(path, output)
    }
}
