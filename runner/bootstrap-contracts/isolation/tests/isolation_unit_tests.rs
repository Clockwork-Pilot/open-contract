use runner_isolation::{Backend, StagedTree};
use runner_manifest::ManifestPlan;
use std::path::Path;

fn package(temp: &Path, name: &str, filesystem: &str, files: &str) -> ManifestPlan {
    std::fs::write(temp.join("dataset.json"), "{\"version\":1,\"cases\":[]}").unwrap();
    std::fs::write(
        temp.join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
            temp.join("contract.yaml"),
            format!("version: '1'\nname: {name}\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\n{files}filesystem: {filesystem}\n"),
        ).unwrap();
    ManifestPlan::load(temp.join("contract.yaml")).unwrap()
}

#[test]
fn isolation_contract_dataset_is_staged_under_a_fresh_root() {
    let dataset: serde_json::Value = serde_json::from_str(include_str!("../dataset.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("dataset.json"), dataset.to_string()).unwrap();
    std::fs::write(
        temp.path().join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
            temp.path().join("contract.yaml"),
            "version: '1'\nname: isolation-fixture\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: [reports]}\n",
        )
        .unwrap();
    let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
    let first = StagedTree::create(&plan).unwrap();
    let second = StagedTree::create(&plan).unwrap();
    assert_ne!(first.root, second.root);
    let staged: serde_json::Value =
        serde_json::from_slice(&std::fs::read(first.root.join("dataset.json")).unwrap()).unwrap();
    assert_eq!(
        staged["cases"][0]["input"]["value"],
        dataset["cases"][0]["input"]["value"]
    );
    assert_eq!(Backend::None.runtime_manifest_name(), None);
    assert_eq!(Backend::Bubblewrap.runtime_manifest_name(), Some("bwrap"));
    assert_eq!(Backend::Landlock.runtime_manifest_name(), Some("landlock"));
}

#[test]
fn manifest_selected_language_adapter_runtime_is_available_without_filesystem_mounts() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = temp.path().join("adapter");
    std::fs::create_dir_all(adapter.join("src")).unwrap();
    std::fs::write(adapter.join("src/adapter.py"), "adapter-runtime-ok").unwrap();
    std::fs::write(
        adapter.join("runtime.yaml"),
        "version: 1\nentrypoint: src/adapter.py\nfiles: [src]\nenvironment_paths: {ADAPTER_RUNTIME_ROOT: src}\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("dataset.json"), "{\"version\":1,\"cases\":[]}").unwrap();
    std::fs::write(temp.path().join("validator.py"), "def validate(value): return value\n").unwrap();
    std::fs::write(
        temp.path().join("contract.yaml"),
        "version: '1'\nname: adapter-runtime-fixture\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nlanguage_adapter: adapter/runtime.yaml\nexecution: {command: [python3, -c, 'import os,pathlib; print(pathlib.Path(os.environ[\"ADAPTER_RUNTIME_ROOT\"], \"adapter.py\").read_text())']}\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
    )
    .unwrap();
    let manifest = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
    let tree = StagedTree::create(&manifest).unwrap();
    let output = tree
        .run_command_at(
            Backend::None,
            &tree.root,
            &manifest.manifest.execution.as_ref().unwrap().command,
            b"",
        )
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "adapter-runtime-ok");
    assert!(!tree.read.iter().any(|path| path.contains("adapter/src")));
}

#[test]
fn isolation_stages_write_files_and_executes_a_command() {
    let temp = tempfile::tempdir().unwrap();
    let plan = package(
        temp.path(),
        "write-fixture",
        "{read: [dataset.json, validator.py], write: [reports/result.json]}",
        "",
    );
    let tree = StagedTree::create(&plan).unwrap();
    assert!(tree.root.join("reports/result.json").is_file());
    let output = tree
        .run_command(
            Backend::None,
            &tree.root,
            &[
                "python3".into(),
                "-c".into(),
                "import sys; print(sys.stdin.read())".into(),
            ],
            b"input-check",
        )
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "input-check"
    );
    assert!(tree
        .run_command(Backend::None, &tree.root, &[], b"")
        .is_err());
}

#[test]
fn isolation_rejects_nested_cycles_and_package_escape_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let cycle = package(
        temp.path(),
        "cycle",
        "{read: [dataset.json, validator.py, contract.yaml], write: []}",
        "nested_contracts: [contract.yaml]\n",
    );
    assert!(StagedTree::create(&cycle)
        .err()
        .unwrap()
        .contains("cycle detected"));

    #[cfg(unix)]
    {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "secret").unwrap();
        std::fs::create_dir(temp.path().join("escape")).unwrap();
        std::fs::write(temp.path().join("escape/child"), "inside").unwrap();
        std::fs::write(temp.path().join("dataset.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("validator.py"),
            "def validate(value): return value\n",
        )
        .unwrap();
        std::fs::write(
                temp.path().join("contract.yaml"),
                "version: '1'\nname: symlink-escape\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py, escape], write: []}\n",
            ).unwrap();
        let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
        std::fs::remove_dir_all(temp.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("escape")).unwrap();
        assert!(StagedTree::create(&plan)
            .err()
            .unwrap()
            .contains("symlink"));
    }
}
