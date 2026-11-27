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
    let setup_case = manifest_setup_case(case.id.as_str(), case.setup.as_deref())?;
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

fn manifest_setup_case<'a>(
    case_id: &str,
    setup: Option<&'a str>,
) -> Result<Option<&'a str>, String> {
    let requires_setup = matches!(
        case_id,
        "read-path-symlink-escape"
            | "declared-file-symlink-escape"
            | "write-path-symlink-escape"
            | "directory-alias-symlink-escape"
            | "coverage-source-symlink-escape"
            | "coverage-glob-dangling-match-is-rejected"
            | "write-path-dangling-symlink"
            | "write-path-inaccessible-ancestor"
            | "coverage-glob-inaccessible-directory"
            | "accepts-relative-read-path-outside-package"
            | "accepts-parent-relative-coverage-source"
            | "accepts-parent-relative-dataset-schema"
            | "dataset-schema-symlink-escape"
            | "nested-contract-sibling-manifest"
            | "nested-contract-multiple-parent-traversals"
            | "nested-contract-absolute-path-rejected"
            | "nested-contract-missing-manifest-rejected"
            | "nested-contract-manifest-not-readable"
            | "nested-contract-symlink-rejected"
            | "nested-contract-empty-component-rejected"
            | "nested-contract-directory-rejected"
            | "nested-contract-file-parent-rejected"
            | "nested-contract-current-directory-component"
    );
    let expected_setup = requires_setup.then_some(case_id);
    if setup != expected_setup {
        return Err(format!(
            "dataset case {case_id:?} setup must be {expected_setup:?}"
        ));
    }
    Ok(setup)
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
    std::fs::create_dir_all(root.join("support/nested")).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("tests")).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("schemas")).map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("schemas/values.yaml"),
        "input: {type: object, fields: {value: integer}}\nexpected: {type: object, fields: {accepted: boolean}}\n",
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(root.join("tests/extra.rs"), "fn supplemental_case() {}\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(root.join("tests/data.json"), "{}\n").map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("output")).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(root.join("child")).map_err(|error| error.to_string())?;
    std::fs::write(root.join("child/contract.yaml"), "version: '1'\n")
        .map_err(|error| error.to_string())?;
    std::fs::create_dir(root.join("directory")).map_err(|error| error.to_string())?;
    std::fs::write(temp.path().join("adapter-runtime.yaml"), "version: 1\n")
        .map_err(|error| error.to_string())?;

    let mut permission_restore = None;
    let mut manifest_root = root.clone();
    match setup_case {
        Some(
            "read-path-symlink-escape"
            | "declared-file-symlink-escape"
            | "dataset-schema-symlink-escape"
            | "directory-alias-symlink-escape",
        ) => {
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
        Some("coverage-source-symlink-escape") => {
            let outside = temp.path().join("outside.py");
            std::fs::write(&outside, "def external(): return True\n")
                .map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, root.join("support/escape.py"))
                .map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&outside, root.join("support/escape.py"))
                .map_err(|error| error.to_string())?;
        }
        Some("coverage-glob-dangling-match-is-rejected") => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                temp.path().join("missing-target.py"),
                root.join("support/dangling.py"),
            )
            .map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(
                temp.path().join("missing-target.py"),
                root.join("support/dangling.py"),
            )
            .map_err(|error| error.to_string())?;
        }
        Some("write-path-inaccessible-ancestor" | "coverage-glob-inaccessible-directory") => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{symlink, PermissionsExt};
                let outside = temp.path().join("private");
                std::fs::create_dir(&outside).map_err(|error| error.to_string())?;
                let permissions = std::fs::Permissions::from_mode(0o000);
                std::fs::set_permissions(&outside, permissions)
                    .map_err(|error| error.to_string())?;
                let link = if setup_case == Some("coverage-glob-inaccessible-directory") {
                    root.join("support/blocked")
                } else {
                    root.join("blocked")
                };
                symlink(&outside, link).map_err(|error| error.to_string())?;
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
        Some("accepts-parent-relative-coverage-source") => {
            std::fs::write(
                temp.path().join("outside.py"),
                "def external(): return True\n",
            )
            .map_err(|error| error.to_string())?;
        }
        Some("accepts-parent-relative-dataset-schema") => {
            std::fs::write(temp.path().join("schema.json"), "{}\n")
                .map_err(|error| error.to_string())?;
        }
        Some(
            "nested-contract-sibling-manifest"
            | "nested-contract-absolute-path-rejected"
            | "nested-contract-missing-manifest-rejected"
            | "nested-contract-manifest-not-readable"
            | "nested-contract-empty-component-rejected"
            | "nested-contract-directory-rejected"
            | "nested-contract-file-parent-rejected"
            | "nested-contract-current-directory-component",
        ) => {
            let sibling = temp.path().join("sibling");
            std::fs::create_dir_all(&sibling).map_err(|error| error.to_string())?;
            if setup_case != Some("nested-contract-missing-manifest-rejected") {
                write_valid_contract_package(&sibling)?;
            }
        }
        Some("nested-contract-symlink-rejected") => {
            let sibling = temp.path().join("sibling");
            std::fs::create_dir_all(&sibling).map_err(|error| error.to_string())?;
            write_valid_contract_package(&sibling)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(sibling.join("contract.yaml"), sibling.join("linked.yaml"))
                .map_err(|error| error.to_string())?;
            #[cfg(not(unix))]
            return Err("nested manifest symlink fixture requires a Unix host".into());
        }
        Some("nested-contract-multiple-parent-traversals") => {
            manifest_root = temp.path().join("level-one/level-two/parent");
            std::fs::create_dir_all(&manifest_root).map_err(|error| error.to_string())?;
            std::fs::write(manifest_root.join("dataset.json"), "{}")
                .map_err(|error| error.to_string())?;
            std::fs::write(
                manifest_root.join("validator.py"),
                "def validate(value): return value\n",
            )
            .map_err(|error| error.to_string())?;
            let sibling = temp.path().join("level-one/contracts/child");
            write_valid_contract_package(&sibling)?;
        }
        None => {}
        Some(other) => return Err(format!("unknown manifest setup case {other:?}")),
    }

    let mut manifest = input["yaml"]
        .as_str()
        .ok_or_else(|| "fixture yaml must be a string".to_string())?
        .to_owned();
    if setup_case == Some("nested-contract-absolute-path-rejected") {
        let child_manifest = temp.path().join("sibling/contract.yaml");
        manifest = manifest.replace("@CHILD_MANIFEST@", &child_manifest.to_string_lossy());
    }
    std::fs::write(manifest_root.join("contract.yaml"), manifest)
        .map_err(|error| error.to_string())?;
    let result = ManifestPlan::load(&manifest_root.join("contract.yaml"));
    if setup_case == Some("nested-contract-missing-manifest-rejected") {
        let error = result
            .as_ref()
            .err()
            .ok_or_else(|| "missing nested manifest was unexpectedly accepted".to_string())?;
        if !error.contains("cannot resolve nested contract manifest") {
            return Err(format!(
                "missing nested manifest error was unclear: {error}"
            ));
        }
    }
    if matches!(
        setup_case,
        Some(
            "nested-contract-manifest-not-readable"
                | "nested-contract-symlink-rejected"
                | "nested-contract-empty-component-rejected"
                | "nested-contract-directory-rejected"
                | "nested-contract-file-parent-rejected"
                | "nested-contract-current-directory-component"
        )
    ) {
        let expected_error = match setup_case {
            Some("nested-contract-manifest-not-readable") => {
                "nested contract manifest is not readable"
            }
            Some("nested-contract-symlink-rejected") => {
                "nested contract manifest path traverses a symlink"
            }
            Some("nested-contract-empty-component-rejected") => {
                "nested contract manifest path has an empty component"
            }
            Some("nested-contract-directory-rejected") => {
                "nested contract manifest is not a regular file"
            }
            Some("nested-contract-file-parent-rejected") => {
                "cannot inspect nested contract manifest"
            }
            Some("nested-contract-current-directory-component") => "",
            _ => unreachable!(),
        };
        let error = result.as_ref().err().map(ToString::to_string);
        if expected_error.is_empty() {
            if error.is_some() {
                return Err(format!(
                    "relative nested manifest with a current-directory component failed: {}",
                    error.unwrap()
                ));
            }
        } else if !error
            .as_deref()
            .is_some_and(|error| error.contains(expected_error))
        {
            return Err(format!(
                "nested manifest setup {setup_case:?} expected error containing {expected_error:?}, got {error:?}"
            ));
        }
    }
    let valid = result.is_ok();
    #[cfg(unix)]
    if let Some(path) = permission_restore {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(valid)
}

fn write_valid_contract_package(root: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    std::fs::write(root.join("dataset.json"), "{}").map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("validator.py"),
        "def validate(value): return value\n",
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("contract.yaml"),
        "version: '1'\nname: child\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "manifest_fixtures.rs"]
mod tests;
