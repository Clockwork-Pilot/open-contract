use open_contract_isolation::validate_language_package;
use open_contract_isolation::{Backend, StagedTree};
use open_contract_manifest::ManifestPlan;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition {
            return Err(format!($($message)*));
        }
    };
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("isolation dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = std::env::args().collect::<Vec<_>>();
    match arguments.get(1).map(String::as_str) {
        Some("--echo-stdin") => {
            io::copy(&mut io::stdin().lock(), &mut io::stdout().lock())
                .map_err(|error| format!("cannot echo command stdin: {error}"))?;
            return Ok(());
        }
        Some("--exit-before-stdin") => return Ok(()),
        Some("--print-cwd-name") => {
            let current = std::env::current_dir()
                .map_err(|error| format!("cannot read command working directory: {error}"))?;
            let name = current
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "command working directory has no usable name".to_string())?;
            println!("{name}");
            return Ok(());
        }
        Some("--read-file") => {
            let path = arguments
                .get(2)
                .ok_or_else(|| "read-file mode requires a path".to_string())?;
            std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            return Ok(());
        }
        _ => {}
    }

    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan = ManifestPlan::load(package.join("contract.yaml"))
        .map_err(|error| format!("cannot load isolation manifest: {error}"))?;
    let dataset_bytes = std::fs::read(package.join("dataset.json"))
        .map_err(|error| format!("cannot read isolation dataset: {error}"))?;
    let dataset: Value = serde_json::from_slice(&dataset_bytes)
        .map_err(|error| format!("invalid isolation dataset JSON: {error}"))?;
    let cases = dataset["cases"]
        .as_array()
        .ok_or_else(|| "isolation dataset must contain a cases array".to_string())?;
    ensure!(
        !cases.is_empty(),
        "dataset must exercise filesystem staging"
    );

    let mut previous_root = None;
    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap_or("unknown");
        let tree = StagedTree::create(&plan)
            .map_err(|error| format!("cases/{id}: cannot stage package: {error}"))?;
        let current_root = tree.root.clone();
        if let Some(previous) = &previous_root {
            ensure!(
                previous != &tree.root,
                "cases/{id}: root was not randomized"
            );
        }
        let input = &case["input"];
        let expected = &case["expected"];
        let operation = input["operation"]
            .as_str()
            .ok_or_else(|| format!("cases/{id}: input.operation must be a string"))?;
        let result = match operation {
            "stage" => check_stage(&tree, &plan, &dataset, expected),
            "reject-cycle-and-package-escape" => check_cycle_and_escape(expected),
            "reject-invalid-staged-paths" => check_invalid_paths(tree, expected),
            "command" => check_command(&tree, input, expected),
            "command-at" => check_command_at(&tree, expected),
            "unknown-language-package" => check_unknown_package(&tree, expected),
            "command-not-found" => check_missing_command(&tree, expected),
            "broken-stdin-pipe" => check_broken_pipe(&tree, expected),
            "missing-bubblewrap" => check_missing_bubblewrap(&tree, expected),
            "mocked-bubblewrap" => check_mocked_bubblewrap(&tree, expected),
            "runtime-mounts-only-adapters" => check_runtime_mounts_only_adapters(&tree, expected),
            "language-package" => check_language_package(case),
            "bubblewrap-invalid-root" => check_bubblewrap_invalid_root(tree, expected),
            "deny-private-read" => check_private_read(&tree, &package, expected),
            "bwrap-command" => check_bubblewrap_command(&tree, expected),
            "empty-command" => check_empty_command(&tree, expected),
            other => Err(format!("cases/{id}: unsupported operation {other:?}")),
        };
        result.map_err(|error| format!("cases/{id}: {error}"))?;
        previous_root = Some(current_root);
        let _ = index;
    }
    println!("Isolation dataset: {} cases passed", cases.len());
    Ok(())
}

fn check_language_package(case: &Value) -> Result<(), String> {
    let id = case["id"].as_str().unwrap_or("unknown");
    let input = &case["input"];
    let language = input["language_package"]
        .as_str()
        .ok_or_else(|| format!("cases/{id}: language_package must be a string"))?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let package = if input["external_package_mapping"] == true {
        temp.path().join("external-language-package")
    } else {
        temp.path().join("contract-languages").join(language)
    };
    std::fs::create_dir_all(&package).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(package.join("src")).map_err(|error| error.to_string())?;
    if input["create_outside_resource"] == true {
        std::fs::write(package.join("outside.py"), "outside runtime resource\n")
            .map_err(|error| error.to_string())?;
    }
    if input["create_symlink_escape"] == true {
        let outside = temp.path().join("outside");
        std::fs::write(&outside, "outside runtime file").map_err(|error| error.to_string())?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, package.join("escape"))
            .map_err(|error| error.to_string())?;
    }
    if input["create_runtime_manifest"] != false {
        let manifest = input["runtime_yaml"]
            .as_str()
            .ok_or_else(|| format!("cases/{id}: runtime_yaml must be a string"))?;
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
    let previous_mapping = std::env::var_os("OPEN_CONTRACT_LANGUAGE_PACKAGES");
    if input["external_package_mapping"] == true {
        let mut mapping = serde_json::Map::new();
        let mapped_path = if input["missing_mapping_target"] == true {
            package.with_file_name("missing-language-package")
        } else {
            package.clone()
        };
        mapping.insert(
            language.to_owned(),
            Value::String(mapped_path.to_string_lossy().into_owned()),
        );
        std::env::set_var(
            "OPEN_CONTRACT_LANGUAGE_PACKAGES",
            Value::Object(mapping).to_string(),
        );
    } else if input["invalid_package_mapping"] == true {
        std::env::set_var("OPEN_CONTRACT_LANGUAGE_PACKAGES", "{");
    } else {
        std::env::remove_var("OPEN_CONTRACT_LANGUAGE_PACKAGES");
    }
    let result = validate_language_package(temp.path(), language);
    if let Some(previous) = previous_mapping {
        std::env::set_var("OPEN_CONTRACT_LANGUAGE_PACKAGES", previous);
    } else {
        std::env::remove_var("OPEN_CONTRACT_LANGUAGE_PACKAGES");
    }
    let expected = &case["expected"];
    match (result, expected["error_contains"].as_str()) {
        (Ok(()), None) if expected["valid"] == true => Ok(()),
        (Err(error), Some(needle)) if error.contains(needle) => Ok(()),
        (Ok(()), _) => Err(format!(
            "cases/{id}: language package unexpectedly validated"
        )),
        (Err(error), _) => Err(format!(
            "cases/{id}: unexpected language package error: {error}"
        )),
    }
}

fn check_stage(
    tree: &StagedTree,
    plan: &ManifestPlan,
    dataset: &Value,
    expected: &Value,
) -> Result<(), String> {
    let staged_bytes = std::fs::read(tree.root.join("dataset.json"))
        .map_err(|error| format!("declared dataset is not staged: {error}"))?;
    let staged: Value = serde_json::from_slice(&staged_bytes)
        .map_err(|error| format!("staged dataset is invalid JSON: {error}"))?;
    ensure!(
        staged == *dataset,
        "staged bytes differ from package dataset"
    );
    ensure!(
        tree.root.join("src/main.rs").is_file(),
        "validator is not staged"
    );
    ensure!(
        tree.root.join("support/helper.py").is_file(),
        "support file is not staged"
    );
    ensure!(
        tree.root.join("nested/dataset.json").is_file(),
        "nested dataset is not staged"
    );
    ensure!(
        tree.root.join("reports").is_dir(),
        "write directory is not created"
    );
    ensure!(
        tree.root.join("reports/result.json").is_file(),
        "write file is not created"
    );
    let existing_result = std::fs::read_to_string(tree.root.join("reports/result.json"))
        .map_err(|error| format!("existing write file was not staged: {error}"))?;
    ensure!(
        existing_result == "{\"existing\":true}\n",
        "existing read/write file was not preserved"
    );
    ensure!(
        tree.root.join("reports/nested/result.json").is_file(),
        "write file with a new parent directory is not created"
    );
    let root_manifest = tree
        .root_manifest()
        .ok_or_else(|| "staged root manifest is missing".to_string())?;
    ensure!(
        root_manifest.manifest.name == plan.manifest.name,
        "wrong root manifest"
    );
    let manifests_with_prefixes = tree
        .manifests_with_prefixes()
        .map(|(manifest, prefix)| (manifest.manifest.name.clone(), prefix.to_path_buf()))
        .collect::<Vec<_>>();
    ensure!(
        manifests_with_prefixes.len() == 2,
        "staged manifest and prefix count is incorrect"
    );
    ensure!(
        manifests_with_prefixes[0].0 == plan.manifest.name
            && manifests_with_prefixes[0].1.as_os_str().is_empty(),
        "root manifest prefix is incorrect"
    );
    ensure!(
        manifests_with_prefixes[1].0 == "isolation-nested-fixture"
            && manifests_with_prefixes[1].1 == Path::new("nested"),
        "nested manifest prefix is incorrect"
    );
    ensure!(Backend::None.as_str() == "none", "backend name mismatch");
    ensure!(
        Backend::Landlock.as_str() == "landlock",
        "backend name mismatch"
    );
    ensure!(
        Backend::Bubblewrap.as_str() == "bwrap",
        "backend name mismatch"
    );
    for (name, actual) in [
        ("dataset_staged", true),
        ("validator_staged", true),
        ("support_staged", true),
        ("nested_dataset_staged", true),
        ("fresh_root", true),
    ] {
        ensure!(expected[name] == actual, "expected.{name} did not match");
    }
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
    let value = input["value"]
        .as_str()
        .ok_or_else(|| "command input value must be a string".to_string())?;
    let backend = match input["backend"].as_str().unwrap_or("none") {
        "none" => Backend::None,
        "landlock" => Backend::Landlock,
        "bwrap" => Backend::Bubblewrap,
        other => return Err(format!("unknown isolation backend {other:?}")),
    };
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = tree.run_command(
        backend,
        &tree.root,
        &[
            executable.to_string_lossy().into_owned(),
            "--echo-stdin".into(),
        ],
        value.as_bytes(),
        input["language_package"].as_str(),
    )?;
    ensure!(output.status.success(), "command failed: {}", output.status);
    ensure!(
        String::from_utf8_lossy(&output.stdout) == expected["stdout"].as_str().unwrap_or_default(),
        "stdout mismatch"
    );
    ensure!(expected["success"] == true, "expected.success is false");
    Ok(())
}

fn check_command_at(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let expected_name = expected["working_directory"]
        .as_str()
        .ok_or_else(|| "working directory expectation must be a string".to_string())?;
    let working_directory = tree.root.join("nested");
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = tree.run_command_at(
        Backend::None,
        &working_directory,
        &[
            executable.to_string_lossy().into_owned(),
            "--print-cwd-name".into(),
        ],
        b"",
        None,
    )?;
    ensure!(output.status.success(), "command failed: {}", output.status);
    ensure!(
        String::from_utf8_lossy(&output.stdout).trim() == expected_name,
        "contract command ran from the wrong package directory"
    );
    let outside = tree.root.parent().ok_or_else(|| "staged root has no parent".to_string())?;
    let error = tree
        .run_command_at(Backend::None, outside, &["true".into()], b"", None)
        .err()
        .ok_or_else(|| "outside working directory was accepted".to_string())?;
    ensure!(
        error.contains("inside the staged contract tree"),
        "unexpected outside working directory error: {error}"
    );
    ensure!(expected["outside_rejected"] == true, "outside rejection expectation is false");
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let error = tree
        .run_command(
            Backend::None,
            &tree.root,
            &[
                executable.to_string_lossy().into_owned(),
                "--exit-before-stdin".into(),
            ],
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

fn check_mocked_bubblewrap(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let result = tree.run_command(
        Backend::Bubblewrap,
        &tree.root,
        &[
            executable.to_string_lossy().into_owned(),
            tree.root.to_string_lossy().into_owned(),
        ],
        b"mocked namespace command",
        None,
    );
    if let Some(path) = previous {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    let output = result?;
    ensure!(
        output.status.success(),
        "mock Bubblewrap launcher failed: {}",
        output.status
    );
    ensure!(
        String::from_utf8_lossy(&output.stdout) == "mocked namespace command",
        "mock launcher stdout mismatch"
    );
    ensure!(
        expected["launcher_invoked"] == true,
        "expected.launcher_invoked is false"
    );
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let private_source = package.join("private-source.txt");
    let output = tree.run_command(
        Backend::Landlock,
        &tree.root,
        &[
            executable.to_string_lossy().into_owned(),
            "--read-file".into(),
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = tree.run_command(
        Backend::Bubblewrap,
        &tree.root,
        &[
            executable.to_string_lossy().into_owned(),
            "--echo-stdin".into(),
        ],
        b"bubblewrap command",
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

fn check_empty_command(tree: &StagedTree, expected: &Value) -> Result<(), String> {
    let error = tree
        .run_command(Backend::None, &tree.root, &[], b"", None)
        .err()
        .ok_or_else(|| "empty command was unexpectedly accepted".to_string())?;
    ensure!(
        error.contains("cannot be empty"),
        "unexpected empty-command error: {error}"
    );
    let package_command_error = tree
        .run_command_at(
            Backend::None,
            &tree.root.join("nested"),
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
    ensure!(expected["package_command_rejected"] == true, "expected.package_command_rejected is false");
    Ok(())
}

fn write(path: impl AsRef<Path>, content: &str) -> Result<(), String> {
    std::fs::write(path.as_ref(), content)
        .map_err(|error| format!("cannot write {}: {error}", path.as_ref().display()))
}
