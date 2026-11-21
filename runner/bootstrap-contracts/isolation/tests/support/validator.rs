use bootstrap_dataset_harness::{run_dataset as run_cases, DatasetCase};
use runner_isolation::{Backend, StagedTree};
use runner_manifest::ManifestPlan;
use serde_json::Value;
use std::path::{Path, PathBuf};

macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition {
            return Err(format!($($message)*));
        }
    };
}

pub(crate) fn execute_dataset() -> Result<usize, String> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let coverage_root = source.join("../../../.coverage/bootstrap-contracts/isolation-fixtures");
    std::fs::create_dir_all(&coverage_root).map_err(|error| error.to_string())?;
    let workspace = tempfile::Builder::new()
        .prefix("dataset-")
        .tempdir_in(&coverage_root)
        .map_err(|error| error.to_string())?;
    let package = workspace
        .path()
        .join("runner/bootstrap-contracts/isolation");
    for relative in [
        "isolation-package.fixture.yaml",
        "dataset.json",
        "Cargo.toml",
        "src/lib.rs",
        "src/backend.rs",
        "support/helper.py",
        "nested/contract.yaml",
        "nested/dataset.json",
        "nested/validator.py",
        "private-source.txt",
    ] {
        let target = package.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::copy(source.join(relative), target).map_err(|error| error.to_string())?;
    }
    // Exercise preservation of existing write files without depending on a prior run.
    for relative in [
        "reports/result.json",
        "reports/nested/result.json",
        "nested/reports/result.json",
    ] {
        let target = package.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(target, "").map_err(|error| error.to_string())?;
    }
    let plan = ManifestPlan::load(package.join("isolation-package.fixture.yaml"))
        .map_err(|error| format!("cannot load isolation manifest: {error}"))?;
    let mut previous_root = None;
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let result = run_cases(package.join("dataset.json"), |_, case| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        let current = validate_case(&plan, &package, &previous_root, case)?;
        previous_root = Some(current);
        #[cfg(coverage)]
        if capture_path.is_some() {
            snapshots.push((case.id.clone(), profile::snapshot()?));
        }
        Ok(())
    });
    #[cfg(coverage)]
    if let Some(path) = capture_path {
        profile::restore_aggregate(&snapshots)?;
        profile::write_snapshots(Path::new(&path), &snapshots)
            .map_err(|error| format!("cannot write coverage snapshots: {error}"))?;
    }
    result
}

#[cfg(coverage)]
#[path = "profile.rs"]
mod profile;

#[path = "dataset_setup.rs"]
mod dataset_setup;

pub(crate) fn validate_case(
    plan: &ManifestPlan,
    package: &Path,
    previous_root: &Option<PathBuf>,
    case: &DatasetCase,
) -> Result<PathBuf, String> {
    if case.input["operation"] == "stage-contract-tree" {
        dataset_setup::run_staging_failure(
            case.id.as_str(),
            case.setup,
            plan,
            &case.input,
            case.expected.as_ref(),
            case.predicate.as_deref(),
        )?;
        return Ok(package.to_path_buf());
    }
    if case.input["operation"] == "stage-backend" {
        dataset_setup::run_staging_backend(
            case.id.as_str(),
            case.setup,
            plan,
            &case.input,
            case.expected.as_ref(),
            case.predicate.as_deref(),
        )?;
        return Ok(package.to_path_buf());
    }
    if case.input["operation"] == "auto-backend" {
        dataset_setup::run_with_case_setup(case.id.as_str(), case.setup, package, || {
            let runtime = case.input["isolation_runtime"].as_str().map(Path::new);
            let (backend, attempts) = runner_isolation::auto_backend(runtime);
            let observed_backend = match backend {
                Backend::TempDir => "tempdir",
                Backend::None => "none",
                Backend::Bubblewrap => "bwrap",
                Backend::Landlock => "landlock",
            };
            let expected = case.expected_value()?;
            ensure!(
                expected["backend"].as_str() == Some(observed_backend),
                "expected auto backend {:?}, observed {observed_backend:?}",
                expected["backend"]
            );
            if let Some(expected_attempts) = expected["failed_probes"].as_u64() {
                ensure!(
                    attempts.len() as u64 == expected_attempts,
                    "expected {expected_attempts} failed probes, observed {}",
                    attempts.len()
                );
            }
            Ok(())
        })?;
        return Ok(package.to_path_buf());
    }
    let tree =
        StagedTree::create(plan).map_err(|error| format!("cannot stage package: {error}"))?;
    let current_root = tree.root.clone();
    if let Some(previous) = previous_root {
        ensure!(previous != &tree.root, "root was not randomized");
    }
    let package_root = tree.root.clone();
    dataset_setup::run_with_case_setup(case.id.as_str(), case.setup, &package_root, || {
        validate_case_with_tree(package, case, tree)
    })?;
    Ok(current_root)
}

fn validate_case_with_tree(
    package: &Path,
    case: &DatasetCase,
    tree: StagedTree,
) -> Result<(), String> {
    let input = &case.input;
    let expected = case.expected_value()?;
    let operation = input["operation"]
        .as_str()
        .ok_or_else(|| "input.operation must be a string".to_string())?;
    match operation {
        "reject-cycle-and-package-escape" => check_cycle_and_escape(expected)?,
        "reject-invalid-staged-paths" => check_invalid_paths(tree, expected)?,
        "command" => check_command(&tree, input, expected)?,
        "command-at" => check_command_at(&tree, input, expected)?,
        "command-not-found" => check_missing_command(&tree, expected)?,
        "broken-stdin-pipe" => check_broken_pipe(&tree, expected)?,
        "missing-bubblewrap" => check_missing_bubblewrap(&tree, expected)?,
        "isolation-runtime-manifest" => check_isolation_runtime_manifest(&tree, expected)?,
        "command-runtime-resolution" => check_command_runtime_resolution(&tree, input, expected)?,
        "bubblewrap-invalid-root" => check_bubblewrap_invalid_root(tree, expected)?,
        "deny-private-read" => check_private_read(&tree, package, expected)?,
        "bwrap-command" => check_bubblewrap_command(&tree, expected)?,
        "empty-command" => check_empty_command(&tree, input, expected)?,
        other => return Err(format!("unsupported operation {other:?}")),
    }
    Ok(())
}

fn check_isolation_runtime_manifest(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let runtime_manifest = temporary.path().join("runtime.yaml");
    std::fs::write(
        &runtime_manifest,
        "version: 1\nread_only: []\nenvironment: {RUNTIME_MARKER: from-runtime-yaml}\ncommand_executable: true\n",
    )
    .map_err(|error| error.to_string())?;

    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = fake_bin.path().join("bwrap");
    std::fs::write(&launcher, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n")
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    let previous_path = std::env::var_os("PATH");
    let fake_path = match &previous_path {
        Some(path) => format!("{}:{}", fake_bin.path().display(), path.to_string_lossy()),
        None => fake_bin.path().display().to_string(),
    };
    std::env::set_var("PATH", fake_path);
    let custom_tree =
        StagedTree::create_with_runtime(&tree.root_plan, Some(runtime_manifest.clone()))?;
    let invocation = custom_tree.run_command(
        Backend::Bubblewrap,
        &custom_tree.root,
        &["true".into()],
        b"",
    );
    if let Some(path) = previous_path {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    let output = invocation?;
    let arguments = String::from_utf8_lossy(&output.stdout);
    ensure!(
        arguments.contains("RUNTIME_MARKER")
            && arguments.contains("from-runtime-yaml")
            && arguments.contains("/usr/bin"),
        "explicit isolation runtime was not applied to Bubblewrap"
    );

    let invalid_manifests = [
        (
            "missing.yaml",
            "version: 1\n".to_string(),
            "cannot read isolation runtime manifest",
        ),
        (
            "invalid.yaml",
            "version: [\n".to_string(),
            "invalid isolation runtime manifest",
        ),
        (
            "unsupported.yaml",
            "version: 2\n".to_string(),
            "unsupported isolation runtime manifest version",
        ),
        (
            "relative.yaml",
            "version: 1\nread_only: [relative]\n".to_string(),
            "paths must be absolute",
        ),
        (
            "unavailable.yaml",
            format!(
                "version: 1\nread_only: [{}]\n",
                temporary.path().join("not-present").display()
            ),
            "isolation runtime path",
        ),
    ];
    for (name, contents, message) in invalid_manifests {
        let path = temporary.path().join(name);
        if name != "missing.yaml" {
            std::fs::write(&path, contents).map_err(|error| error.to_string())?;
        }
        let custom_tree = StagedTree::create_with_runtime(&tree.root_plan, Some(path))?;
        let error = custom_tree
            .run_command(
                Backend::Bubblewrap,
                &custom_tree.root,
                &["true".into()],
                b"",
            )
            .err()
            .ok_or_else(|| format!("invalid isolation runtime {name} was accepted"))?;
        ensure!(
            error.contains(message),
            "unexpected isolation runtime error: {error}"
        );
    }
    ensure!(
        expected["override_applied"] == true,
        "expected.override_applied is false"
    );
    ensure!(
        expected["invalid_manifests_rejected"] == true,
        "expected.invalid_manifests_rejected is false"
    );
    exercise_language_adapter_runtime_manifest(expected)?;
    Ok(())
}

fn exercise_language_adapter_runtime_manifest(expected: &Value) -> Result<(), String> {
    let package = tempfile::tempdir().map_err(|error| error.to_string())?;
    let adapter = package.path().join("adapter");
    std::fs::create_dir_all(&adapter).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(package.path().join("src")).map_err(|error| error.to_string())?;
    write(package.path().join("dataset.yaml"), "cases: []\n")?;
    write(package.path().join("src/api.rs"), "pub fn api() {}\n")?;
    write(adapter.join("helper.py"), "# adapter fixture\n")?;
    write(adapter.join("other.py"), "# undeclared adapter fixture\n")?;
    let manifest = package.path().join("contract.yaml");
    write(
        &manifest,
        "version: '1'\nname: language-adapter-fixture\ndatasets: [dataset.yaml]\ncoverage_sources: [src/api.rs]\nlanguage_adapter: adapter/runtime.yaml\nfilesystem: {read: [dataset.yaml, src, adapter], write: []}\n",
    )?;
    let runtime_manifest = adapter.join("runtime.yaml");
    let valid_runtime = "version: 1\nentrypoint: helper.py\nfiles: [helper.py]\nenvironment: {OPEN_CONTRACT_LANGUAGE_RUNTIME_MARKER: loaded}\nenvironment_paths: {OPEN_CONTRACT_LANGUAGE_RUNTIME_FILE: helper.py}\n";
    write(&runtime_manifest, valid_runtime)?;
    let plan = ManifestPlan::load(&manifest)
        .map_err(|error| format!("cannot load language adapter fixture contract: {error}"))?;
    let tree = StagedTree::create_for_backend(&plan, Backend::None, None)?;
    let output = tree.run_command_at(
        Backend::None,
        &tree.root,
        &["/usr/bin/env".into()],
        b"",
    )?;
    ensure!(output.status.success(), "language adapter command failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    ensure!(
        stdout.contains("OPEN_CONTRACT_LANGUAGE_RUNTIME_MARKER=loaded")
            && stdout.contains(&format!(
                "OPEN_CONTRACT_LANGUAGE_ENTRYPOINT={}",
                adapter.join("helper.py").display()
            ))
            && stdout.contains(&format!(
                "OPEN_CONTRACT_LANGUAGE_RUNTIME_FILE={}",
                adapter.join("helper.py").display()
            )),
        "language adapter entrypoint, files, or environment were not applied"
    );

    let expect_error = |contents: &str, needle: &str| -> Result<(), String> {
        write(&runtime_manifest, contents)?;
        let error = tree
            .plan_command_at(Backend::None, &tree.root, &["true".into()])
            .err()
            .ok_or_else(|| format!("language adapter runtime was accepted: {contents:?}"))?;
        ensure!(error.contains(needle), "unexpected language adapter error: {error}");
        Ok(())
    };

    let checks = (|| {
        std::fs::remove_file(&runtime_manifest).map_err(|error| error.to_string())?;
        let missing_error = tree
            .plan_command_at(Backend::None, &tree.root, &["true".into()])
            .err()
            .ok_or_else(|| "missing language adapter runtime manifest was accepted".to_string())?;
        ensure!(
            missing_error.contains("cannot resolve language adapter runtime manifest"),
            "unexpected missing runtime manifest error: {missing_error}"
        );
        write(&runtime_manifest, valid_runtime)?;

        std::fs::remove_file(&runtime_manifest).map_err(|error| error.to_string())?;
        std::fs::create_dir(&runtime_manifest).map_err(|error| error.to_string())?;
        let read_error = tree
            .plan_command_at(Backend::None, &tree.root, &["true".into()])
            .err()
            .ok_or_else(|| "directory language adapter runtime manifest was accepted".to_string())?;
        ensure!(
            read_error.contains("cannot read language adapter runtime manifest"),
            "unexpected unreadable runtime manifest error: {read_error}"
        );
        std::fs::remove_dir(&runtime_manifest).map_err(|error| error.to_string())?;

        expect_error("version: [\n", "invalid language adapter runtime manifest")?;
        expect_error("version: 2\nentrypoint: helper.py\nfiles: [helper.py]\n", "unsupported language adapter runtime manifest version")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: []\n", "declares no files")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: [../helper.py]\n", "paths must stay inside")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: [/tmp/helper.py]\n", "paths must stay inside")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: [missing.py]\n", "is unavailable")?;
        expect_error("version: 1\nentrypoint: missing.py\nfiles: [helper.py]\n", "is unavailable")?;
        expect_error("version: 1\nentrypoint: other.py\nfiles: [helper.py]\n", "entrypoint must be a file included")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: [helper.py]\nenvironment_paths: {MISSING: missing.py}\n", "is unavailable")?;
        expect_error("version: 1\nentrypoint: helper.py\nfiles: [helper.py]\nenvironment_paths: {UNDECLARED: other.py}\n", "outside declared files")?;

        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().map_err(|error| error.to_string())?;
            write(outside.path().join("outside.py"), "# outside\n")?;
            std::os::unix::fs::symlink(
                outside.path().join("outside.py"),
                adapter.join("escape.py"),
            )
            .map_err(|error| error.to_string())?;
            expect_error("version: 1\nentrypoint: escape.py\nfiles: [escape.py]\n", "escapes its package")?;
        }
        Ok(())
    })();
    let restore = write(&runtime_manifest, valid_runtime);
    checks?;
    restore?;
    ensure!(
        expected["language_adapter_runtime_applied"] == true,
        "expected.language_adapter_runtime_applied is false"
    );
    ensure!(
        expected["invalid_language_adapter_manifests_rejected"] == true,
        "expected.invalid_language_adapter_manifests_rejected is false"
    );
    Ok(())
}

fn check_command_runtime_resolution(
    tree: &StagedTree,
    input: &Value,
    expected: &Value,
) -> Result<(), String> {
    let runtime_manifest = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    let contents = input["runtime_yaml"]
        .as_str()
        .ok_or_else(|| "input.runtime_yaml must be a string".to_string())?;
    std::fs::write(runtime_manifest.path(), contents).map_err(|error| error.to_string())?;
    let custom_tree = StagedTree::create_with_runtime(
        &tree.root_plan,
        Some(runtime_manifest.path().to_path_buf()),
    )?;
    let command = input["command"]
        .as_array()
        .ok_or_else(|| "input.command must be an array".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "input.command entries must be strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let error = custom_tree
        .run_command(Backend::Bubblewrap, &custom_tree.root, &command, b"")
        .err()
        .ok_or_else(|| "runtime accepted an unresolved command executable".to_string())?;
    let needle = expected["error_contains"]
        .as_str()
        .ok_or_else(|| "expected.error_contains must be a string".to_string())?;
    ensure!(error.contains(needle), "unexpected command error: {error}");
    Ok(())
}

fn check_cycle_and_escape(expected: &Value) -> Result<(), String> {
    let cycle_root = tempfile::tempdir().map_err(|error| error.to_string())?;
    write(cycle_root.path().join("dataset.json"), "{}")?;
    write(
        cycle_root.path().join("validator.py"),
        "def validate(x): return x\n",
    )?;
    write(
        cycle_root.path().join("contract.yaml"),
        "version: '1'\nname: cycle\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nnested_contracts: [contract.yaml]\nfilesystem: {read: [dataset.json, validator.py, contract.yaml], write: []}\n",
    )?;
    let cycle = ManifestPlan::load(cycle_root.path().join("contract.yaml"))
        .map_err(|error| format!("cycle fixture manifest: {error}"))?;
    let cycle_error = StagedTree::create(&cycle)
        .err()
        .ok_or_else(|| "nested cycle was unexpectedly accepted".to_string())?;
    ensure!(
        cycle_error.contains("cycle detected"),
        "unexpected cycle error: {cycle_error}"
    );

    #[cfg(unix)]
    {
        let package = tempfile::tempdir().map_err(|error| error.to_string())?;
        let outside = tempfile::tempdir().map_err(|error| error.to_string())?;
        write(package.path().join("dataset.json"), "{}")?;
        write(
            package.path().join("validator.py"),
            "def validate(x): return x\n",
        )?;
        std::fs::create_dir(package.path().join("escape")).map_err(|error| error.to_string())?;
        write(package.path().join("escape/child"), "inside")?;
        write(outside.path().join("secret"), "secret")?;
        write(
            package.path().join("contract.yaml"),
            "version: '1'\nname: symlink-escape\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py, escape], write: []}\n",
        )?;
        let plan = ManifestPlan::load(package.path().join("contract.yaml"))
            .map_err(|error| format!("symlink fixture manifest: {error}"))?;
        std::fs::remove_dir_all(package.path().join("escape"))
            .map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(outside.path(), package.path().join("escape"))
            .map_err(|error| error.to_string())?;
        let error = StagedTree::create(&plan)
            .err()
            .ok_or_else(|| "package escape symlink was unexpectedly accepted".to_string())?;
        ensure!(
            error.contains("escapes its package"),
            "unexpected package escape error: {error}"
        );
    }
    ensure!(
        expected["cycle_rejected"] == true,
        "cycle expectation is invalid"
    );
    ensure!(
        expected["package_escape_rejected"] == true,
        "escape expectation is invalid"
    );
    Ok(())
}

fn check_invalid_paths(mut tree: StagedTree, expected: &Value) -> Result<(), String> {
    tree.read.push("..".into());
    let invalid = tree
        .run_command(Backend::Landlock, &tree.root, &["true".into()], b"")
        .err()
        .ok_or_else(|| "relative parent path was unexpectedly accepted".to_string())?;
    ensure!(
        invalid.contains("staged path escapes package root"),
        "unexpected path error: {invalid}"
    );
    tree.read.pop();
    ensure!(
        expected["relative_parent_rejected"] == true,
        "relative parent path expectation is invalid"
    );
    Ok(())
}

fn check_command(tree: &StagedTree, input: &Value, expected: &Value) -> Result<(), String> {
    let backend_name = input["backend"]
        .as_str()
        .ok_or_else(|| "input.backend must select an isolation backend".to_string())?;
    let backend = match backend_name {
        "none" => Backend::None,
        "landlock" => Backend::Landlock,
        "bwrap" => Backend::Bubblewrap,
        other => return Err(format!("unknown isolation backend {other:?}")),
    };
    let command = input
        .get("command")
        .ok_or_else(|| "input.command is required".to_string())?
        .as_array()
        .ok_or_else(|| "command input command must be an array".to_string())?
        .iter()
        .map(|argument| {
            argument
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "command input arguments must be strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let stdin = input["stdin"].as_str().unwrap_or_default();
    let result = tree.run_command(
        backend,
        &tree.root,
        &command,
        stdin.as_bytes(),
    );
    if let Some(needle) = expected["error_contains"].as_str() {
        let error = result
            .err()
            .ok_or_else(|| format!("expected command error containing {needle:?}"))?;
        ensure!(error.contains(needle), "unexpected command error: {error}");
        return Ok(());
    }
    let output = result?;
    if expected["success"] == false {
        ensure!(
            !output.status.success(),
            "expected command failure, got successful exit"
        );
        if let Some(code) = expected["exit_code"].as_i64() {
            ensure!(
                output.status.code() == Some(code as i32),
                "expected command exit code {code}, got {:?}",
                output.status.code()
            );
        }
        return Ok(());
    }
    ensure!(
        output.status.success(),
        "command failed: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(expected_stdout) = expected["stdout"].as_str() {
        ensure!(stdout == expected_stdout, "stdout mismatch");
    }
    if let Some(expected_contains) = expected["stdout_contains"].as_str() {
        ensure!(
            stdout.contains(expected_contains),
            "stdout did not contain {expected_contains:?}"
        );
    }
    if expected["stdout_contains_staged_subpath"] == true {
        let root_name = tree
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "staged root has no usable directory name".to_string())?;
        ensure!(
            stdout.contains(&format!("/contract/{root_name}/support")),
            "package-relative command argument was not mapped into the Bubblewrap root"
        );
    }
    ensure!(expected["success"] == true, "expected.success is false");
    Ok(())
}

fn check_command_at(tree: &StagedTree, input: &Value, expected: &Value) -> Result<(), String> {
    let working_directory = input["working_directory"]
        .as_str()
        .ok_or_else(|| "input.working_directory must be a string".to_string())?;
    let working_directory = tree.root.join(working_directory);
    if let Some(needle) = expected["error_contains"].as_str() {
        let error = tree
            .run_command_at(
                Backend::None,
                &working_directory,
                &["/bin/pwd".into()],
                b"",
            )
            .err()
            .ok_or_else(|| format!("expected working-directory error containing {needle:?}"))?;
        ensure!(
            error.contains(needle),
            "unexpected working-directory error: {error}"
        );
        return Ok(());
    }
    let backend = match input["backend"].as_str().unwrap_or("none") {
        "none" => Backend::None,
        "landlock" => Backend::Landlock,
        "bwrap" => Backend::Bubblewrap,
        other => return Err(format!("unknown isolation backend {other:?}")),
    };
    let command = input["command"]
        .as_array()
        .map(|arguments| {
            arguments
                .iter()
                .map(|argument| {
                    argument
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "command input arguments must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_else(|| vec!["/bin/pwd".into()]);
    let output = tree.run_command_at(backend, &working_directory, &command, b"")?;
    ensure!(output.status.success(), "command failed: {}", output.status);
    if expected["success"] == true {
        return Ok(());
    }
    let expected_name = expected["working_directory"]
        .as_str()
        .ok_or_else(|| "expected.working_directory must be a string".to_string())?;
    ensure!(
        Path::new(String::from_utf8_lossy(&output.stdout).trim())
            .file_name()
            .and_then(|name| name.to_str())
            == Some(expected_name),
        "contract command ran from the wrong package directory"
    );
    let outside = tree
        .root
        .parent()
        .ok_or_else(|| "staged root has no parent".to_string())?;
    let error = tree
        .run_command_at(Backend::None, outside, &["true".into()], b"")
        .err()
        .ok_or_else(|| "outside working directory was accepted".to_string())?;
    ensure!(
        error.contains("inside the staged contract tree"),
        "unexpected outside working directory error: {error}"
    );
    ensure!(
        expected["outside_rejected"] == true,
        "outside rejection expectation is false"
    );
    Ok(())
}

fn check_missing_command(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(
            Backend::Landlock,
            &tree.root,
            &["open-contract-command-that-does-not-exist".into()],
            b"",
        )
        .err()
        .ok_or_else(|| "missing executable was unexpectedly accepted".to_string())?;
    ensure!(!error.is_empty(), "missing executable error was empty");
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    Ok(())
}

fn check_broken_pipe(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(
            Backend::None,
            &tree.root,
            &["/bin/true".into()],
            &vec![b'x'; 4 * 1024 * 1024],
        )
        .err()
        .ok_or_else(|| "closed command stdin pipe was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("failed writing command input"),
        "unexpected stdin error: {error}"
    );
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    Ok(())
}

fn check_missing_bubblewrap(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let previous = std::env::var_os("PATH");
    std::env::set_var("PATH", "");
    let result = tree.run_command(Backend::Bubblewrap, &tree.root, &["true".into()], b"");
    if let Some(path) = previous {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    let error = result
        .err()
        .ok_or_else(|| "missing bwrap was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("bwrap is not installed"),
        "unexpected bwrap error: {error}"
    );
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    Ok(())
}

fn check_bubblewrap_invalid_root(mut tree: StagedTree, expected: &Value) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    tree.read.clear();
    tree.write.clear();
    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = fake_bin.path().join("bwrap");
    write(&launcher, "#!/bin/sh\ncat\n")?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let previous = std::env::var_os("PATH");
    let path = match &previous {
        Some(old) => format!("{}:{}", fake_bin.path().display(), old.to_string_lossy()),
        None => fake_bin.path().display().to_string(),
    };
    std::env::set_var("PATH", path);
    let result = tree.run_command(
        Backend::Bubblewrap,
        Path::new("/"),
        &["true".into()],
        b"",
    );
    if let Some(path) = previous {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    let error = result
        .err()
        .ok_or_else(|| "path without package name was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("randomized root name"),
        "unexpected guest-root error: {error}"
    );
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    Ok(())
}

fn check_private_read(tree: &StagedTree, package: &Path, expected: &Value) -> Result<(), String> {
    let private_source = package.join("private-source.txt");
    let output = tree.run_command(
        Backend::Landlock,
        &tree.root,
        &[
            "/bin/cat".into(),
            private_source.to_string_lossy().into_owned(),
        ],
        b"",
    )?;
    ensure!(
        output.status.code() == Some(1),
        "private file read unexpectedly succeeded: {}",
        output.status
    );
    ensure!(expected["denied"] == true, "expected.denied is false");
    Ok(())
}

fn check_bubblewrap_command(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let output = tree.run_command(
        Backend::Bubblewrap,
        &tree.root,
        &["/bin/echo".into(), "bubblewrap command".into()],
        b"",
    )?;
    if output.status.success() {
        ensure!(
            String::from_utf8_lossy(&output.stdout) == "bubblewrap command",
            "Bubblewrap stdout mismatch"
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            stderr.contains("No permissions to create new namespace"),
            "unexpected Bubblewrap failure: {stderr}"
        );
        ensure!(
            expected["namespace_unavailable"] == true,
            "expected namespace-unavailable outcome"
        );
    }
    ensure!(expected["attempted"] == true, "expected.attempted is false");
    Ok(())
}

fn check_empty_command(tree: &StagedTree, input: &Value, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(Backend::None, &tree.root, &[], b"")
        .err()
        .ok_or_else(|| "empty command was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("cannot be empty"),
        "unexpected empty-command error: {error}"
    );
    let working_directory = input["working_directory"]
        .as_str()
        .ok_or_else(|| "input.working_directory must be a string".to_string())?;
    let package_command_error = tree
        .run_command_at(
            Backend::None,
            &tree.root.join(working_directory),
            &[],
            b"",
        )
        .err()
        .ok_or_else(|| "empty package command was unexpectedly accepted".to_string())?;
    ensure!(
        package_command_error.contains("cannot be empty"),
        "unexpected package-command error: {package_command_error}"
    );
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    ensure!(
        expected["package_command_rejected"] == true,
        "expected.package_command_rejected is false"
    );
    Ok(())
}

fn write(path: impl AsRef<Path>, content: &str) -> Result<(), String> {
    std::fs::write(path.as_ref(), content)
        .map_err(|error| format!("cannot write {}: {error}", path.as_ref().display()))
}
