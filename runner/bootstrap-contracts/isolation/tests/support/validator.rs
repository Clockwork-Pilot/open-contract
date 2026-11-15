use bootstrap_dataset_harness::{run_dataset as run_cases, DatasetCase};
use runner_isolation::validate_language_package;
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
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan = ManifestPlan::load(package.join("isolation-package.fixture.yaml"))
        .map_err(|error| format!("cannot load isolation manifest: {error}"))?;
    let mut previous_root = None;
    run_cases(package.join("dataset.json"), |_, case| {
        let current = validate_case(&plan, &package, &previous_root, case)?;
        previous_root = Some(current);
        Ok(())
    })
}

#[path = "dataset_setup.rs"]
mod dataset_setup;

pub(crate) fn validate_case(
    plan: &ManifestPlan,
    package: &Path,
    previous_root: &Option<PathBuf>,
    case: &DatasetCase,
) -> Result<PathBuf, String> {
    if case.input["operation"] == "stage-contract-tree" {
        let expected = case.expected_value()?;
        dataset_setup::run_staging_failure(
            case.id.as_str(),
            case.setup,
            plan,
            &case.input,
            expected,
        )?;
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
        "unknown-language-package" => check_unknown_package(&tree, expected)?,
        "command-not-found" => check_missing_command(&tree, expected)?,
        "broken-stdin-pipe" => check_broken_pipe(&tree, expected)?,
        "missing-bubblewrap" => check_missing_bubblewrap(&tree, expected)?,
        "runtime-mounts-only-adapters" => check_runtime_mounts_only_adapters(&tree, expected)?,
        "isolation-runtime-manifest" => check_isolation_runtime_manifest(&tree, expected)?,
        "command-runtime-resolution" => check_command_runtime_resolution(&tree, input, expected)?,
        "language-package" => check_language_package(case.id.as_str(), input, expected)?,
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
        None,
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
                None,
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
        .run_command(Backend::Bubblewrap, &custom_tree.root, &command, b"", None)
        .err()
        .ok_or_else(|| "runtime accepted an unresolved command executable".to_string())?;
    let needle = expected["error_contains"]
        .as_str()
        .ok_or_else(|| "expected.error_contains must be a string".to_string())?;
    ensure!(error.contains(needle), "unexpected command error: {error}");
    Ok(())
}

fn check_language_package(case_id: &str, input: &Value, expected: &Value) -> Result<(), String> {
    let language = input["language_package"]
        .as_str()
        .ok_or_else(|| "language_package must be a string".to_string())?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let package = if input["external_package_mapping"] == true {
        temp.path().join("external-language-package")
    } else {
        temp.path().join("contract-languages").join(language)
    };
    std::fs::create_dir_all(&package).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(package.join("src")).map_err(|error| error.to_string())?;
    if std::env::var_os("OPEN_CONTRACT_ISOLATION_CREATE_UNDECLARED_LANGUAGE_RESOURCE").is_some() {
        std::fs::write(package.join("outside.py"), "outside runtime resource\n")
            .map_err(|error| error.to_string())?;
    }
    if case_id == "language-package-rejects-symlink-escape" {
        let outside = temp.path().join("outside");
        std::fs::write(&outside, "outside runtime file").map_err(|error| error.to_string())?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, package.join("escape"))
            .map_err(|error| error.to_string())?;
    }
    if std::env::var_os("OPEN_CONTRACT_ISOLATION_OMIT_LANGUAGE_RUNTIME_MANIFEST").is_none() {
        let manifest = input["runtime_yaml"]
            .as_str()
            .ok_or_else(|| "runtime_yaml must be a string".to_string())?;
        std::fs::write(package.join("runtime.yaml"), manifest)
            .map_err(|error| error.to_string())?;
    }
    if input["create_entrypoint"] != false {
        let entrypoint = input["entrypoint_path"].as_str().unwrap_or("src/runner.py");
        let path = package.join(entrypoint);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, "# runtime entrypoint\n").map_err(|error| error.to_string())?;
    }
    if std::env::var_os("OPEN_CONTRACT_ISOLATION_CREATE_UNDECLARED_LANGUAGE_ENTRYPOINT").is_some() {
        std::fs::write(
            package.join("src/not-present.py"),
            "# undeclared entrypoint\n",
        )
        .map_err(|error| error.to_string())?;
    }
    let previous_mapping = std::env::var_os("OPEN_CONTRACT_LANGUAGE_PACKAGES");
    if previous_mapping.is_none() {
        let mut mapping = serde_json::Map::new();
        mapping.insert(
            language.to_owned(),
            Value::String(package.to_string_lossy().into_owned()),
        );
        std::env::set_var(
            "OPEN_CONTRACT_LANGUAGE_PACKAGES",
            Value::Object(mapping).to_string(),
        );
    }
    let result = validate_language_package(temp.path(), language);
    if let Some(previous) = previous_mapping {
        std::env::set_var("OPEN_CONTRACT_LANGUAGE_PACKAGES", previous);
    } else {
        std::env::remove_var("OPEN_CONTRACT_LANGUAGE_PACKAGES");
    }
    match (result, expected["error_contains"].as_str()) {
        (Ok(()), None) if expected["valid"] == true => Ok(()),
        (Err(error), Some(needle)) if error.contains(needle) => Ok(()),
        (Ok(()), _) => Err(format!("language package unexpectedly validated")),
        (Err(error), _) => Err(format!("unexpected language package error: {error}")),
    }
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
        "version: '1'\nname: cycle\nfiles: {dataset: dataset.json, validator: validator.py, nested_contracts: [contract.yaml]}\nfilesystem: {read: [dataset.json, validator.py, contract.yaml], write: []}\n",
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
            "version: '1'\nname: symlink-escape\nfiles: {dataset: dataset.json, validator: validator.py}\nfilesystem: {read: [dataset.json, validator.py, escape], write: []}\n",
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
            error.contains("escapes package"),
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
    tree.read.push("../outside".into());
    let invalid = tree
        .run_command(Backend::Landlock, &tree.root, &["true".into()], b"", None)
        .err()
        .ok_or_else(|| "parent traversal was unexpectedly accepted".to_string())?;
    ensure!(
        invalid.contains("invalid staged path"),
        "unexpected path error: {invalid}"
    );
    tree.read.pop();
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().map_err(|error| error.to_string())?;
        write(outside.path().join("secret"), "secret")?;
        std::os::unix::fs::symlink(outside.path(), tree.root.join("escape-through-symlink"))
            .map_err(|error| error.to_string())?;
        tree.read.push("escape-through-symlink".into());
        let escaped = tree
            .run_command(Backend::Landlock, &tree.root, &["true".into()], b"", None)
            .err()
            .ok_or_else(|| "resolved package escape was unexpectedly accepted".to_string())?;
        ensure!(
            escaped.contains("escapes package root"),
            "unexpected escape error: {escaped}"
        );
    }
    ensure!(
        expected["parent_traversal_rejected"] == true,
        "parent traversal expectation is invalid"
    );
    ensure!(
        expected["symlink_escape_rejected"] == true,
        "symlink escape expectation is invalid"
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
        input["language_package"].as_str(),
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
                None,
            )
            .err()
            .ok_or_else(|| format!("expected working-directory error containing {needle:?}"))?;
        ensure!(
            error.contains(needle),
            "unexpected working-directory error: {error}"
        );
        return Ok(());
    }
    let expected_name = expected["working_directory"]
        .as_str()
        .ok_or_else(|| "expected.working_directory must be a string".to_string())?;
    let output = tree.run_command_at(
        Backend::None,
        &working_directory,
        &["/bin/pwd".into()],
        b"",
        None,
    )?;
    ensure!(output.status.success(), "command failed: {}", output.status);
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
        .run_command_at(Backend::None, outside, &["true".into()], b"", None)
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

fn check_unknown_package(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(
            Backend::None,
            &tree.root,
            &["true".into()],
            b"",
            Some("unsupported"),
        )
        .err()
        .ok_or_else(|| "unknown language package was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("language package"),
        "unexpected package error: {error}"
    );
    ensure!(expected["rejected"] == true, "expected.rejected is false");
    Ok(())
}

fn check_missing_command(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(
            Backend::Landlock,
            &tree.root,
            &["open-contract-command-that-does-not-exist".into()],
            b"",
            None,
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
            None,
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
    let result = tree.run_command(Backend::Bubblewrap, &tree.root, &["true".into()], b"", None);
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

fn check_runtime_mounts_only_adapters(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = fake_bin.path().join("bwrap");
    write(&launcher, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n")?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let previous = std::env::var_os("PATH");
    let path = match &previous {
        Some(old) => format!("{}:{}", fake_bin.path().display(), old.to_string_lossy()),
        None => fake_bin.path().display().to_string(),
    };
    std::env::set_var("PATH", path);
    let results = ["python", "javascript"]
        .iter()
        .map(|package| {
            tree.run_command(
                Backend::Bubblewrap,
                &tree.root,
                &["true".into()],
                b"",
                Some(package),
            )
            .map(|output| output.stdout)
        })
        .collect::<Result<Vec<_>, _>>();
    if let Some(path) = previous {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    let output = results?;
    let arguments = String::from_utf8_lossy(&output.concat()).into_owned();
    ensure!(
        arguments.contains("contract-languages/python/src"),
        "Python language support was not mounted"
    );
    ensure!(
        arguments.contains("contract-languages/javascript/src"),
        "JavaScript language support was not mounted"
    );
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    for relative in [
        "examples/python",
        "contract-languages/python/schemas",
        "runner/bootstrap-contracts/predicates/predicate-language.schema.json",
    ] {
        let path = repository.join(relative);
        ensure!(
            !arguments
                .lines()
                .any(|argument| argument == path.to_string_lossy()),
            "undeclared runtime path was included in Bubblewrap arguments: {}",
            path.display()
        );
    }
    for relative in expected["javascript_runtime_packages"]
        .as_array()
        .ok_or_else(|| "expected.javascript_runtime_packages must be an array".to_string())?
    {
        let relative = relative
            .as_str()
            .ok_or_else(|| "JavaScript runtime package path must be a string".to_string())?;
        let path = repository.join(relative);
        ensure!(
            arguments
                .lines()
                .any(|argument| argument == path.to_string_lossy()),
            "declared JavaScript runtime package was not mounted: {}",
            path.display()
        );
    }
    ensure!(
        expected["adapter_roots_only"] == true,
        "expected.adapter_roots_only is false"
    );
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
        None,
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
        None,
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
        None,
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
        .run_command(Backend::None, &tree.root, &[], b"", None)
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
            None,
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
