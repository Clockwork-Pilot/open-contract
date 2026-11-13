use open_contract_manifest::{validate_package_path, ManifestPlan};
use serde_yaml::Value;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("manifest dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(package.join("dataset.yaml"))
        .map_err(|error| format!("cannot read dataset: {error}"))?;
    let dataset: Value =
        serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid dataset YAML: {error}"))?;
    let cases = dataset["cases"]
        .as_sequence()
        .ok_or_else(|| "dataset must contain a cases sequence".to_string())?;
    if cases.is_empty() {
        return Err("dataset must exercise manifest behavior".into());
    }

    for case in cases {
        let id = case["id"].as_str().unwrap_or("unknown");
        let operation = case["input"]["operation"]
            .as_str()
            .ok_or_else(|| format!("cases/{id}: operation must be a string"))?;
        let expected = case["expected"]["valid"]
            .as_bool()
            .ok_or_else(|| format!("cases/{id}: expected.valid must be boolean"))?;
        let valid = match operation {
            "path" => {
                let path = case["input"]["path"]
                    .as_str()
                    .ok_or_else(|| format!("cases/{id}: path must be a string"))?;
                validate_package_path(path).is_ok()
            }
            "manifest" => run_manifest_case(case)?,
            "missing-manifest" => {
                let missing = package.join("file-that-does-not-exist.yaml");
                ManifestPlan::load(missing).is_ok()
            }
            "directory-manifest" => {
                let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
                ManifestPlan::load(directory.path()).is_ok()
            }
            "root-manifest" => ManifestPlan::load("/").is_ok(),
            other => return Err(format!("cases/{id}: unknown operation {other:?}")),
        };
        if valid != expected {
            return Err(format!(
                "cases/{id}: expected valid={expected}, got {valid}"
            ));
        }
    }
    println!("Manifest dataset: {} cases passed", cases.len());
    Ok(())
}

fn run_manifest_case(case: &Value) -> Result<bool, String> {
    let temp =
        tempfile::tempdir().map_err(|error| format!("fixture package directory: {error}"))?;
    let root = temp.path().join("package");
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    std::fs::write(root.join("dataset.json"), "{}").map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("validator.py"),
        "def validate(value): return value\n",
    )
    .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("support")).map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("support/helper.py"),
        "def helper(): return True\n",
    )
    .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("tests")).map_err(|error| error.to_string())?;
    std::fs::write(root.join("tests/extra.rs"), "fn supplemental_case() {}\n")
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("child")).map_err(|error| error.to_string())?;
    std::fs::write(root.join("child/contract.yaml"), "version: '1'\n")
        .map_err(|error| error.to_string())?;
    std::fs::create_dir(root.join("directory")).map_err(|error| error.to_string())?;

    if case["input"]["symlink_escape"].as_bool() == Some(true) {
        let outside = temp.path().join("outside.json");
        std::fs::write(&outside, "{}").map_err(|error| error.to_string())?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("escape"))
            .map_err(|error| error.to_string())?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, root.join("escape"))
            .map_err(|error| error.to_string())?;
    }

    let manifest = case["input"]["yaml"]
        .as_str()
        .ok_or_else(|| "fixture yaml must be a string".to_string())?;
    std::fs::write(root.join("contract.yaml"), manifest).map_err(|error| error.to_string())?;
    Ok(ManifestPlan::load(root.join("contract.yaml")).is_ok())
}
