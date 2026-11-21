#[derive(Clone, Copy)]
enum CaseSetup {
    AutoBubblewrapSucceeds,
    AutoBubblewrapFails,
    AutoBubblewrapMissing,
    AutoBubblewrapFailsWithMessage,
    AutoLandlockFails,
    AutoNoTrueExecutable,
    AutoNoTemporaryDirectory,
    UnusableTemporaryDirectory,
    HideBubblewrapFromPath,
    MockBubblewrapLauncher,
    PrintBubblewrapArguments,
    SetEnvironmentValue,
    ClearEnvironmentValue,
    InvalidUtf8EnvironmentValue,
    SetEmptyEnvironmentValue,
    ClearLangEnvironmentValue,
    LandlockInstallationProbe,
    BackendErrorPaths,
    StagingCopyErrorPaths,
    RemoveStagedRoot,
    RemoveStagedSupport,
    RemoveStagedWriteFile,
    BubblewrapPackageRoot,
    StageBackendBubblewrapArguments,
    ReplaceWriteParentWithFile,
    ReplaceWriteDirectoryWithFile,
    RelativeCommandExecutable,
    LandlockOpenRulePathFailure,
    LandlockAbiQueryFailure,
    LandlockRulesetCreateFailure,
    LandlockAddPathRuleFailure,
    LandlockSetNoNewPrivilegesFailure,
    LandlockRestrictSelfFailure,
}

const CASE_SETUP_BY_ID: &[(&str, CaseSetup)] = &[
    (
        "auto-backend-prefers-bubblewrap",
        CaseSetup::AutoBubblewrapSucceeds,
    ),
    (
        "auto-backend-falls-back-to-landlock",
        CaseSetup::AutoBubblewrapFails,
    ),
    (
        "auto-backend-falls-back-to-tempdir",
        CaseSetup::AutoBubblewrapFails,
    ),
    (
        "auto-backend-falls-back-when-landlock-fails",
        CaseSetup::AutoLandlockFails,
    ),
    (
        "auto-backend-reports-missing-bubblewrap",
        CaseSetup::AutoBubblewrapMissing,
    ),
    (
        "auto-backend-falls-back-after-bubblewrap-error-output",
        CaseSetup::AutoBubblewrapFailsWithMessage,
    ),
    (
        "auto-backend-falls-back-when-true-is-missing",
        CaseSetup::AutoNoTrueExecutable,
    ),
    (
        "auto-backend-falls-back-to-none",
        CaseSetup::AutoNoTemporaryDirectory,
    ),
    (
        "staged-tree-rejects-unusable-temporary-directory",
        CaseSetup::UnusableTemporaryDirectory,
    ),
    ("bubblewrap-unavailable", CaseSetup::HideBubblewrapFromPath),
    (
        "bubblewrap-missing-from-path",
        CaseSetup::HideBubblewrapFromPath,
    ),
    (
        "bubblewrap-command-stdin-and-stdout",
        CaseSetup::MockBubblewrapLauncher,
    ),
    (
        "bubblewrap-command-returns-nonzero-status",
        CaseSetup::MockBubblewrapLauncher,
    ),
    (
        "bubblewrap-command-maps-package-root",
        CaseSetup::BubblewrapPackageRoot,
    ),
    (
        "stage-backend-bwrap-uses-source-root",
        CaseSetup::StageBackendBubblewrapArguments,
    ),
    (
        "stage-backend-bwrap-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "direct-backend-reports-write-parent-error",
        CaseSetup::ReplaceWriteParentWithFile,
    ),
    (
        "direct-backend-reports-write-directory-error",
        CaseSetup::ReplaceWriteDirectoryWithFile,
    ),
    (
        "command-expands-environment-reference",
        CaseSetup::SetEnvironmentValue,
    ),
    (
        "command-executes-relative-path-from-contract-directory",
        CaseSetup::RelativeCommandExecutable,
    ),
    (
        "command-rejects-unset-environment-reference",
        CaseSetup::ClearEnvironmentValue,
    ),
    (
        "command-rejects-invalid-utf8-environment-reference",
        CaseSetup::InvalidUtf8EnvironmentValue,
    ),
    (
        "landlock-uses-default-language-environment",
        CaseSetup::ClearLangEnvironmentValue,
    ),
    (
        "landlock-installation-failure-scenarios",
        CaseSetup::LandlockInstallationProbe,
    ),
    ("command-backend-error-paths", CaseSetup::BackendErrorPaths),
    ("staging-copy-error-paths", CaseSetup::StagingCopyErrorPaths),
    (
        "command-rejects-empty-expanded-executable",
        CaseSetup::SetEmptyEnvironmentValue,
    ),
    (
        "command-none-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-bwrap-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-landlock-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-bwrap-reports-missing-staged-support",
        CaseSetup::RemoveStagedSupport,
    ),
    (
        "command-landlock-reports-missing-staged-support",
        CaseSetup::RemoveStagedSupport,
    ),
    (
        "command-bwrap-reports-missing-staged-write-file",
        CaseSetup::RemoveStagedWriteFile,
    ),
    (
        "command-landlock-reports-missing-staged-write-file",
        CaseSetup::RemoveStagedWriteFile,
    ),
    (
        "command-at-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "direct-backend-reports-missing-source-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "direct-backend-at-reports-missing-source-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "landlock-open-rule-path-failure",
        CaseSetup::LandlockOpenRulePathFailure,
    ),
    (
        "landlock-abi-query-failure",
        CaseSetup::LandlockAbiQueryFailure,
    ),
    (
        "landlock-ruleset-create-failure",
        CaseSetup::LandlockRulesetCreateFailure,
    ),
    (
        "landlock-add-path-rule-failure",
        CaseSetup::LandlockAddPathRuleFailure,
    ),
    (
        "landlock-set-no-new-privileges-failure",
        CaseSetup::LandlockSetNoNewPrivilegesFailure,
    ),
    (
        "landlock-restrict-self-failure",
        CaseSetup::LandlockRestrictSelfFailure,
    ),
];

pub(super) fn run_staging_failure(
    case_id: &str,
    setup_enabled: bool,
    plan: &runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: Option<&serde_json::Value>,
    predicate: Option<&str>,
) -> Result<(), String> {
    let mut plan = plan.clone();
    let temporary = if setup_enabled {
        if case_id != "staged-tree-rejects-unusable-temporary-directory" {
            return Err(format!(
                "dataset case {case_id:?} has unexpected setup: true"
            ));
        }
        let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let not_a_directory = temporary.path().join("not-a-directory");
        std::fs::write(&not_a_directory, "file").map_err(|error| error.to_string())?;
        let _temporary_directory =
            EnvironmentGuard::set("TMPDIR", not_a_directory.into_os_string());
        let result = run_staging_case(&mut plan, input, expected, predicate);
        drop(_temporary_directory);
        return result;
    } else {
        if case_id == "staged-tree-rejects-unusable-temporary-directory" {
            return Err(format!("dataset case {case_id:?} must declare setup: true"));
        }
        None::<tempfile::TempDir>
    };
    let _temporary = temporary;
    run_staging_case(&mut plan, input, expected, predicate)
}

fn run_staging_case(
    plan: &mut runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: Option<&serde_json::Value>,
    predicate: Option<&str>,
) -> Result<(), String> {
    apply_staging_manifest_input(plan, input)?;
    match runner_isolation::StagedTree::create(plan) {
        Ok(tree) => {
            let expected =
                expected.ok_or_else(|| "staging succeeded without expected output".to_string())?;
            if expected["created"] != true {
                return Err("staging succeeded but expected.created was not true".into());
            }
            let writes = &input["manifest"]["filesystem"]["write"];
            if let Some(writes) = writes.as_array() {
                for path in writes {
                    let path = path
                        .as_str()
                        .ok_or_else(|| "filesystem.write items must be strings".to_string())?;
                    if !tree.root.join(path).exists() {
                        return Err(format!("declared writable path {path:?} was not staged"));
                    }
                }
            }
            Ok(())
        }
        Err(error) => {
            if let Some(predicate) = predicate {
                let actual = serde_json::json!({ "error": error });
                let expected = expected
                    .ok_or_else(|| "public staging error has no expected result".to_string())?;
                if &actual != expected {
                    return Err(format!(
                        "public staging result mismatch; expected: {expected}; actual: {actual}"
                    ));
                }
                if runner_predicates::evaluate_predicate(&actual, predicate) {
                    return Ok(());
                }
                return Err(format!(
                    "public staging error did not satisfy predicate; actual: {actual}"
                ));
            }
            let needle = expected
                .and_then(|value| value["error_contains"].as_str())
                .ok_or_else(|| "staging failed without an expected error predicate".to_string())?;
            if error.contains(needle) {
                Ok(())
            } else {
                Err(format!("unexpected staging error: {error}"))
            }
        }
    }
}

pub(super) fn run_staging_backend(
    case_id: &str,
    setup_enabled: bool,
    root_plan: &runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: Option<&serde_json::Value>,
    predicate: Option<&str>,
) -> Result<(), String> {
    if matches!(
        case_id,
        "direct-backend-prepares-write-paths"
            | "direct-backend-reports-missing-source-root"
            | "direct-backend-at-reports-missing-source-root"
            | "direct-backend-reports-write-parent-error"
            | "direct-backend-reports-write-directory-error"
            | "stage-backend-bwrap-reports-missing-root"
    ) {
        if !setup_enabled {
            return Err(format!("dataset case {case_id:?} must declare setup: true"));
        }
        let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let package = temporary.path().join("package");
        std::fs::create_dir_all(&package).map_err(|error| error.to_string())?;
        std::fs::write(package.join("dataset.json"), "{}").map_err(|error| error.to_string())?;
        std::fs::write(
            package.join("validator.py"),
            "def validate(value): return value\n",
        )
        .map_err(|error| error.to_string())?;
        std::fs::write(
            package.join("contract.yaml"),
            "version: '1'\nname: direct-write-fixture\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
        )
        .map_err(|error| error.to_string())?;
        let plan = runner_manifest::ManifestPlan::load(package.join("contract.yaml"))?;
        return stage_backend_case(case_id, setup_enabled, &plan, input, expected, predicate);
    }
    if matches!(
        case_id,
        "tempdir-read-rejects-recursive-symlink-escape"
            | "tempdir-nested-contract-rejects-symlink-escape"
    ) {
        if !setup_enabled {
            return Err(format!("dataset case {case_id:?} must declare setup: true"));
        }
        let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let package = temporary.path().join("package");
        let external = temporary.path().join("external");
        std::fs::create_dir_all(&package).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&external).map_err(|error| error.to_string())?;
        let mut plan = root_plan.clone();
        plan.package_root = package.clone();
        if case_id == "tempdir-read-rejects-recursive-symlink-escape" {
            let source = package.join("source");
            std::fs::create_dir_all(&source).map_err(|error| error.to_string())?;
            std::fs::write(external.join("outside.txt"), "outside")
                .map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(external.join("outside.txt"), source.join("escape"))
                .map_err(|error| error.to_string())?;
            #[cfg(not(unix))]
            return Err("recursive symlink fixture requires a Unix host".into());
        } else {
            let nested = package.join("nested");
            std::fs::create_dir_all(&nested).map_err(|error| error.to_string())?;
            std::fs::write(package.join("dataset.json"), "{}")
                .map_err(|error| error.to_string())?;
            std::fs::write(external.join("dataset.json"), "{}")
                .map_err(|error| error.to_string())?;
            std::fs::write(
                external.join("validator.py"),
                "def validate(value): return value\n",
            )
            .map_err(|error| error.to_string())?;
            std::fs::write(
                external.join("contract.yaml"),
                "version: '1'\nname: external-child\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
            )
            .map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                external.join("contract.yaml"),
                nested.join("contract.yaml"),
            )
            .map_err(|error| error.to_string())?;
            #[cfg(not(unix))]
            return Err("nested symlink fixture requires a Unix host".into());
        }
        return stage_backend_case(case_id, false, &plan, input, expected, predicate);
    }
    stage_backend_case(
        case_id,
        setup_enabled,
        root_plan,
        input,
        expected,
        predicate,
    )
}

fn stage_backend_case(
    case_id: &str,
    setup_enabled: bool,
    root_plan: &runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: Option<&serde_json::Value>,
    predicate: Option<&str>,
) -> Result<(), String> {
    let mut plan = root_plan.clone();
    apply_staging_manifest_input(&mut plan, input)?;
    let backend = match input["backend"].as_str() {
        Some("tempdir") => runner_isolation::Backend::TempDir,
        Some("none") => runner_isolation::Backend::None,
        Some("bwrap") => runner_isolation::Backend::Bubblewrap,
        Some("landlock") => runner_isolation::Backend::Landlock,
        Some(other) => return Err(format!("unsupported staging backend {other:?}")),
        None => return Err("input.backend must be a string".into()),
    };
    let isolation_runtime = input["isolation_runtime"]
        .as_str()
        .map(std::path::PathBuf::from);
    let tree =
        match runner_isolation::StagedTree::create_for_backend(&plan, backend, isolation_runtime) {
            Ok(tree) => tree,
            Err(error) => {
                let actual = serde_json::json!({"error": error});
                let expected = expected
                    .ok_or_else(|| "staging failed without an expected output".to_string())?;
                return match_staging_output(&actual, expected, predicate);
            }
        };
    if let Some(command) = input["command"].as_array() {
        let command = command
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "input.command items must be strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let run_command = || {
            if let Some(working_directory) = input["working_directory"].as_str() {
                tree.run_command_at(
                    backend,
                    &tree.root.join(working_directory),
                    &command,
                    b"",
                )
            } else {
                tree.run_command(backend, &tree.root, &command, b"")
            }
        };
        let output = if case_id == "direct-backend-prepares-write-paths" {
            run_command()
        } else {
            run_with_case_setup(case_id, setup_enabled, &tree.root, run_command)
        };
        let actual = match output {
            Ok(output) => serde_json::json!({
                "success": output.status.success(),
                "stdout": String::from_utf8_lossy(&output.stdout),
            }),
            Err(error) => serde_json::json!({"error": error}),
        };
        let expected =
            expected.ok_or_else(|| "command completed without an expected output".to_string())?;
        return match_staging_output(&actual, expected, predicate);
    }
    if setup_enabled {
        return Err(format!(
            "dataset case {case_id:?} declares unexpected setup: true"
        ));
    }
    let actual = serde_json::json!({
        "source_root_used": tree.root == plan.package_root,
        "read_paths_absolute": tree.read.iter().all(|path| std::path::Path::new(path).is_absolute()),
        "write_paths_absolute": tree.write.iter().all(|path| std::path::Path::new(path).is_absolute()),
        "nested_manifest_count": tree.manifests.len(),
        "nested_manifest_exists": tree.root.join("nested/contract.yaml").is_file(),
        "runtime_manifest_name": backend.runtime_manifest_name(),
    });
    let expected =
        expected.ok_or_else(|| "staging succeeded without an expected output".to_string())?;
    match_staging_output(&actual, expected, predicate)
}

fn match_staging_output(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    predicate: Option<&str>,
) -> Result<(), String> {
    let expected_fields = expected
        .as_object()
        .ok_or_else(|| "expected staging result must be an object".to_string())?;
    let actual_fields = actual
        .as_object()
        .ok_or_else(|| "actual staging result must be an object".to_string())?;
    for (field, value) in expected_fields {
        if actual_fields.get(field) != Some(value) {
            return Err(format!(
                "staging result field {field:?} mismatch; expected {value}, actual {}",
                actual_fields
                    .get(field)
                    .map_or_else(|| "<missing>".to_string(), serde_json::Value::to_string)
            ));
        }
    }
    if let Some(predicate) = predicate {
        if !runner_predicates::evaluate_predicate(actual, predicate) {
            return Err(format!(
                "staging result did not satisfy predicate {predicate:?}; actual: {actual}"
            ));
        }
    }
    Ok(())
}

fn apply_staging_manifest_input(
    plan: &mut runner_manifest::ManifestPlan,
    input: &serde_json::Value,
) -> Result<(), String> {
    if input["operation"] != "stage-contract-tree" && input["operation"] != "stage-backend" {
        return Err("input.operation must be stage-contract-tree or stage-backend".into());
    }
    let manifest = input["manifest"]
        .as_object()
        .ok_or_else(|| "input.manifest must be an object".to_string())?;
    let mut has_manifest_input = false;
    if let Some(filesystem) = manifest.get("filesystem") {
        let filesystem = filesystem
            .as_object()
            .ok_or_else(|| "input.manifest.filesystem must be an object".to_string())?;
        reject_unknown_keys(filesystem, &["read", "write"], "input.manifest.filesystem")?;
        if let Some(read) = filesystem.get("read") {
            plan.manifest.filesystem.read = string_list(read, "input.manifest.filesystem.read")?;
            has_manifest_input = true;
        }
        if let Some(write) = filesystem.get("write") {
            plan.manifest.filesystem.write = string_list(write, "input.manifest.filesystem.write")?;
            has_manifest_input = true;
        }
    }
    if let Some(nested_contracts) = manifest.get("nested_contracts") {
        plan.manifest.nested_contracts =
            string_list(nested_contracts, "input.manifest.nested_contracts")?;
        has_manifest_input = true;
    }
    reject_unknown_keys(manifest, &["filesystem", "nested_contracts"], "input.manifest")?;
    let allowed_input_keys: &[&str] = if input["operation"] == "stage-backend" {
        &[
            "operation",
            "backend",
            "command",
            "working_directory",
            "isolation_runtime",
            "manifest",
        ]
    } else {
        &["operation", "manifest"]
    };
    reject_unknown_keys(
        input.as_object().expect("validated operation object"),
        allowed_input_keys,
        "input",
    )?;
    if !has_manifest_input {
        return Err("input.manifest must specify filesystem.read, filesystem.write, or nested_contracts".into());
    }
    Ok(())
}

fn reject_unknown_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
    field: &str,
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{field} contains unsupported field {key:?}"));
    }
    Ok(())
}

fn string_list(value: &serde_json::Value, field: &str) -> Result<Vec<String>, String> {
    let values = value
        .as_array()
        .ok_or_else(|| format!("{field} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{field} items must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(format!("{field} must contain at least one path"));
    }
    Ok(values)
}

pub(super) fn run_with_case_setup<T: Send>(
    case_id: &str,
    setup_enabled: bool,
    package_root: &std::path::Path,
    run: impl FnOnce() -> Result<T, String> + Send,
) -> Result<T, String> {
    let setup = CASE_SETUP_BY_ID
        .iter()
        .find_map(|(id, setup)| (*id == case_id).then_some(*setup));
    match (setup_enabled, setup) {
        (false, Some(_)) => Err(format!(
            "dataset case {case_id:?} has test setup but does not declare setup: true"
        )),
        (true, None) => Err(format!(
            "dataset case {case_id:?} declares setup: true but has no test setup"
        )),
        (true, Some(CaseSetup::UnusableTemporaryDirectory)) => Err(format!(
            "dataset case {case_id:?} setup must run before staging"
        )),
        (true, Some(CaseSetup::AutoBubblewrapSucceeds)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nexit 0\n", run)
        }
        (true, Some(CaseSetup::AutoBubblewrapFails)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nexit 125\n", run)
        }
        (true, Some(CaseSetup::AutoBubblewrapMissing)) => run_with_true_only_path(run),
        (true, Some(CaseSetup::AutoBubblewrapFailsWithMessage)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\necho probe-failed >&2\nexit 125\n", run)
        }
        (true, Some(CaseSetup::AutoNoTrueExecutable)) => {
            let _path = EnvironmentGuard::clear("PATH");
            run()
        }
        (true, Some(CaseSetup::AutoLandlockFails)) => with_landlock_filter(
            LandlockFilter::AbiQuery,
            || run_with_bubblewrap_launcher("#!/bin/sh\nexit 125\n", run),
        ),
        (true, Some(CaseSetup::AutoNoTemporaryDirectory)) => {
            let blocker = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
            run_with_bubblewrap_launcher("#!/bin/sh\nexit 125\n", || {
                let _tmpdir =
                    EnvironmentGuard::set("TMPDIR", blocker.path().as_os_str().to_owned());
                run()
            })
        }
        (true, Some(CaseSetup::HideBubblewrapFromPath)) => {
            let _path = EnvironmentGuard::clear("PATH");
            run()
        }
        (true, Some(CaseSetup::MockBubblewrapLauncher)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then shift; exec \"$@\"; fi\n  shift\ndone\nexit 125\n", run)
        }
        (true, Some(CaseSetup::BubblewrapPackageRoot)) => {
            let _root = EnvironmentGuard::set(
                "OPEN_CONTRACT_TEST_PACKAGE_ROOT",
                package_root.as_os_str().to_owned(),
            );
            run_with_bubblewrap_launcher(
                "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then shift; exec \"$@\"; fi\n  shift\ndone\nexit 125\n",
                run,
            )
        }
        (true, Some(CaseSetup::PrintBubblewrapArguments)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nprintf '%s\\n' \"$@\"\n", run)
        }
        (true, Some(CaseSetup::StageBackendBubblewrapArguments)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nprintf '%s\\n' \"$@\"\n", run)
        }
        (true, Some(CaseSetup::SetEnvironmentValue)) => {
            let _value = EnvironmentGuard::set("OPEN_CONTRACT_TEST_VALUE", "contract supplied value");
            run()
        }
        (true, Some(CaseSetup::RelativeCommandExecutable)) => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                "/usr/bin/true",
                package_root.join("support/contract-command"),
            )
            .map_err(|error| format!("cannot create relative executable fixture: {error}"))?;
            #[cfg(not(unix))]
            return Err("relative executable fixture requires a Unix host".into());
            run()
        }
        (true, Some(CaseSetup::ClearEnvironmentValue)) => {
            let _value = EnvironmentGuard::clear("OPEN_CONTRACT_TEST_UNSET");
            run()
        }
        (true, Some(CaseSetup::InvalidUtf8EnvironmentValue)) => {
            use std::os::unix::ffi::OsStringExt;
            let _value = EnvironmentGuard::set(
                "OPEN_CONTRACT_TEST_INVALID_UTF8",
                std::ffi::OsString::from_vec(vec![0xff]),
            );
            run()
        }
        (true, Some(CaseSetup::SetEmptyEnvironmentValue)) => {
            let _value = EnvironmentGuard::set("OPEN_CONTRACT_TEST_EMPTY", "   ");
            run()
        }
        (true, Some(CaseSetup::LandlockInstallationProbe)) => {
            crate::test_fixture::exercise_landlock_installation_paths()?;
            run()
        }
        (true, Some(CaseSetup::BackendErrorPaths)) => {
            crate::test_fixture::exercise_backend_error_paths()?;
            run()
        }
        (true, Some(CaseSetup::StagingCopyErrorPaths)) => {
            crate::test_fixture::exercise_staging_copy_error_paths()?;
            run()
        }
        (true, Some(CaseSetup::ClearLangEnvironmentValue)) => {
            let _lang = EnvironmentGuard::clear("LANG");
            run()
        }
        (true, Some(CaseSetup::RemoveStagedRoot)) => {
            std::fs::remove_dir_all(package_root).map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::ReplaceWriteParentWithFile)) => {
            let parent = package_root.join("setup/write-parent");
            std::fs::create_dir_all(parent.parent().expect("write parent has a parent"))
                .map_err(|error| error.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink("write-parent", parent)
                .map_err(|error| error.to_string())?;
            #[cfg(not(unix))]
            return Err("write parent failure fixture requires a Unix host".into());
            run()
        }
        (true, Some(CaseSetup::ReplaceWriteDirectoryWithFile)) => {
            let target = package_root.join("setup/write-directory");
            std::fs::create_dir_all(target.parent().expect("write directory has a parent"))
                .map_err(|error| error.to_string())?;
            std::fs::write(target, "blocking file").map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::RemoveStagedSupport)) => {
            std::fs::remove_dir_all(package_root.join("support"))
                .map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::RemoveStagedWriteFile)) => {
            std::fs::remove_file(package_root.join("reports/result.json"))
                .map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::LandlockOpenRulePathFailure)) => {
            with_landlock_filter(LandlockFilter::OpenRulePath, run)
        }
        (true, Some(CaseSetup::LandlockAbiQueryFailure)) => {
            with_landlock_filter(LandlockFilter::AbiQuery, run)
        }
        (true, Some(CaseSetup::LandlockRulesetCreateFailure)) => {
            with_landlock_filter(LandlockFilter::CreateRuleset, run)
        }
        (true, Some(CaseSetup::LandlockAddPathRuleFailure)) => {
            with_landlock_filter(LandlockFilter::AddPathRule, run)
        }
        (true, Some(CaseSetup::LandlockSetNoNewPrivilegesFailure)) => {
            with_landlock_filter(LandlockFilter::SetNoNewPrivileges, run)
        }
        (true, Some(CaseSetup::LandlockRestrictSelfFailure)) => {
            with_landlock_filter(LandlockFilter::RestrictSelf, run)
        }
        (false, None) => run(),
    }
}

#[derive(Clone, Copy)]
enum LandlockFilter {
    OpenRulePath,
    AbiQuery,
    CreateRuleset,
    AddPathRule,
    SetNoNewPrivileges,
    RestrictSelf,
}

fn with_landlock_filter<T, F>(filter: LandlockFilter, run: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> Result<T, String> + Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                install_landlock_filter(filter)?;
                run()
            })
            .join()
            .map_err(|_| "Landlock setup thread panicked".to_string())?
    })
}

#[cfg(target_os = "linux")]
fn install_landlock_filter(filter: LandlockFilter) -> Result<(), String> {
    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    const BPF_LD_W_ABS: u16 = 0x20;
    const BPF_JMP_JEQ_K: u16 = 0x15;
    const BPF_RET_K: u16 = 0x06;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    const SYS_LANDLOCK_CREATE_RULESET: u32 = 444;
    const SYS_LANDLOCK_ADD_RULE: u32 = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: u32 = 446;
    const PR_SET_SECCOMP: libc::c_int = 22;
    const SECCOMP_MODE_FILTER: libc::c_int = 2;
    const O_PATH: u32 = 0x0020_0000;
    const O_CLOEXEC: u32 = 0x0008_0000;

    fn statement(code: u16, k: u32) -> SockFilter {
        SockFilter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }
    fn jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
        SockFilter { code, jt, jf, k }
    }

    let deny = SECCOMP_RET_ERRNO | libc::EPERM as u32;
    let allow = statement(BPF_RET_K, SECCOMP_RET_ALLOW);
    let deny_rule = statement(BPF_RET_K, deny);
    let instructions = match filter {
        LandlockFilter::OpenRulePath => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, libc::SYS_openat as u32, 0, 3),
            statement(BPF_LD_W_ABS, 32),
            jump(BPF_JMP_JEQ_K, O_PATH | O_CLOEXEC, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::AbiQuery => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_CREATE_RULESET, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::CreateRuleset => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_CREATE_RULESET, 0, 3),
            statement(BPF_LD_W_ABS, 16),
            jump(BPF_JMP_JEQ_K, 0, 1, 0),
            deny_rule,
            allow,
        ],
        LandlockFilter::AddPathRule => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_ADD_RULE, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::SetNoNewPrivileges => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, libc::SYS_prctl as u32, 0, 3),
            statement(BPF_LD_W_ABS, 16),
            jump(BPF_JMP_JEQ_K, libc::PR_SET_NO_NEW_PRIVS as u32, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::RestrictSelf => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_RESTRICT_SELF, 0, 1),
            deny_rule,
            allow,
        ],
    };
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } < 0 {
        return Err(format!(
            "cannot enable seccomp setup: {}",
            std::io::Error::last_os_error()
        ));
    }
    let program = SockFprog {
        len: instructions.len() as u16,
        filter: instructions.as_ptr(),
    };
    let result = unsafe {
        libc::prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &program as *const SockFprog,
        )
    };
    if result < 0 {
        return Err(format!(
            "cannot install seccomp setup: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn install_landlock_filter(_filter: LandlockFilter) -> Result<(), String> {
    Err("Landlock dataset setup requires Linux seccomp".into())
}

fn run_with_bubblewrap_launcher<T>(
    content: &str,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    use std::os::unix::fs::PermissionsExt;
    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = fake_bin.path().join("bwrap");
    std::fs::write(&launcher, content).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let path = match std::env::var_os("PATH") {
        Some(previous) => {
            let mut paths = vec![fake_bin.path().to_path_buf()];
            paths.extend(std::env::split_paths(&previous));
            std::env::join_paths(paths).map_err(|error| error.to_string())?
        }
        None => fake_bin.path().as_os_str().to_owned(),
    };
    let _path = EnvironmentGuard::set("PATH", path);
    run()
}

fn run_with_true_only_path<T>(run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use std::os::unix::fs::symlink;
    let true_executable = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("true"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "cannot locate true for the auto-backend setup".to_string())?;
    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    symlink(true_executable, fake_bin.path().join("true")).map_err(|error| error.to_string())?;
    let _path = EnvironmentGuard::set("PATH", fake_bin.path().as_os_str().to_owned());
    run()
}

struct EnvironmentGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentGuard {
    fn set(name: &'static str, value: impl Into<std::ffi::OsString>) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value.into());
        Self { name, previous }
    }

    fn clear(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}
