use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_manifest::{is_allowed, validate_package_path, ManifestPlan};
use runner_verification::match_output;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::ExitCode;

pub fn entrypoint() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("manifest dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

pub fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let cases = run_dataset(package.join("dataset.yaml"), |_, case| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        let result = validate_case(&package, case);
        #[cfg(coverage)]
        if capture_path.is_some() {
            let bytes = profile::snapshot()?;
            snapshots.push((case.id.clone(), bytes));
        }
        result
    })?;
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)?;
        profile::write_snapshots(PathBuf::from(path).as_path(), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    println!("Manifest dataset: {cases} cases passed");
    Ok(())
}

#[cfg(coverage)]
#[path = "profile.rs"]
mod profile;

fn validate_case(package: &PathBuf, case: &DatasetCase) -> Result<(), String> {
    let input = &case.input;
    let setup_case = manifest_setup_case(case.id.as_str(), case.setup)?;
    let operation = input["operation"]
        .as_str()
        .ok_or_else(|| "input.operation must be a string".to_string())?;
    let valid = match operation {
        "path" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            validate_package_path(path).is_ok()
        }
        "allowlist" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            let allowed = input["allowed"]
                .as_array()
                .ok_or_else(|| "input.allowed must be an array".to_string())?
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "input.allowed entries must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            is_allowed(path, &allowed)
        }
        "manifest" => run_manifest_case(input, setup_case)?,
        "missing-manifest" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            ManifestPlan::load(&package.join(path)).is_ok()
        }
        "directory-manifest" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            ManifestPlan::load(&package.join(path)).is_ok()
        }
        "root-manifest" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            ManifestPlan::load(std::path::Path::new(path)).is_ok()
        }
        "string-manifest" => {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| "input.path must be a string".to_string())?;
            ManifestPlan::load(path).is_ok()
        }
        other => return Err(format!("unknown operation {other:?}")),
    };
    let actual = json!({"valid": valid});
    match_output(&actual, case.expected.as_ref(), case.predicate.as_deref()).map(|_| ())
}

fn manifest_setup_case(case_id: &str, setup: bool) -> Result<Option<&str>, String> {
    let requires_setup = matches!(
        case_id,
        "read-path-symlink-escape"
            | "declared-file-symlink-escape"
            | "write-path-symlink-escape"
            | "write-path-dangling-symlink"
            | "write-path-inaccessible-ancestor"
            | "accepts-relative-read-path-outside-package"
    );
    if setup != requires_setup {
        return Err(format!(
            "dataset case {case_id:?} setup must be {requires_setup}"
        ));
    }
    Ok(requires_setup.then_some(case_id))
}

fn run_manifest_case(input: &Value, setup_case: Option<&str>) -> Result<bool, String> {
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

    let mut permission_restore = None;
    match setup_case {
        Some("read-path-symlink-escape" | "declared-file-symlink-escape") => {
            let outside = temp.path().join("outside.json");
            std::fs::write(&outside, "{}").map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, root.join("escape"))
                .map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&outside, root.join("escape"))
                .map_err(|error| error.to_string())?;
        }
        Some("write-path-symlink-escape") => {
            let outside = temp.path().join("outside");
            std::fs::create_dir(&outside).map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, root.join("escape"))
                .map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&outside, root.join("escape"))
                .map_err(|error| error.to_string())?;
        }
        Some("write-path-inaccessible-ancestor") => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{symlink, PermissionsExt};
                let outside = temp.path().join("private");
                std::fs::create_dir(&outside).map_err(|error| error.to_string())?;
                let permissions = std::fs::Permissions::from_mode(0o000);
                std::fs::set_permissions(&outside, permissions)
                    .map_err(|error| error.to_string())?;
                symlink(&outside, root.join("blocked")).map_err(|error| error.to_string())?;
                permission_restore = Some(outside);
            }
        }
        Some("write-path-dangling-symlink") => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(temp.path().join("missing-target"), root.join("dangling"))
                .map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(
                temp.path().join("missing-target"),
                root.join("dangling"),
            )
            .map_err(|error| error.to_string())?;
        }
        Some("accepts-relative-read-path-outside-package") => {
            std::fs::write(temp.path().join("outside.json"), "{}")
                .map_err(|error| error.to_string())?;
        }
        None => {}
        Some(other) => return Err(format!("unknown manifest setup case {other:?}")),
    }

    let manifest = input["yaml"]
        .as_str()
        .ok_or_else(|| "fixture yaml must be a string".to_string())?;
    std::fs::write(root.join("contract.yaml"), manifest).map_err(|error| error.to_string())?;
    let valid = ManifestPlan::load(&root.join("contract.yaml")).is_ok();
    #[cfg(unix)]
    if let Some(path) = permission_restore {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(valid)
}

#[cfg(test)]
#[path = "../manifest_fixtures.rs"]
mod tests;
