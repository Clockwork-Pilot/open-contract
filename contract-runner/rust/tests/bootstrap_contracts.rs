use open_contract_core::batch::{
    AdapterBatch, AdapterResult, BatchAdapter, ContractBatch, ExecutionRequest,
};
use open_contract_core::verify::{match_output, MatchKind};
use open_contract_core::{
    run_contract_tree, Backend, ManifestPlan, RuntimeBatchAdapter, StagedTree,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct EchoBatch {
    calls: Mutex<Vec<(String, Vec<ExecutionRequest>)>>,
    invocations: AtomicUsize,
}

impl BatchAdapter for EchoBatch {
    fn execute_tree(
        &self,
        _tree: &StagedTree,
        _backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().extend(
            batches
                .iter()
                .map(|batch| (batch.manifest.manifest.name.clone(), batch.requests.clone())),
        );
        Ok(batches
            .iter()
            .map(|batch| AdapterBatch {
                results: batch
                    .requests
                    .iter()
                    .map(|request| AdapterResult {
                        key: request.key.clone(),
                        output: Some(request.input.clone()),
                        error: None,
                    })
                    .collect(),
                coverage: Some(json!({
                    "native": "fixture",
                    "contract": batch.manifest.manifest.name,
                    "sources": [batch.package_prefix.join(&batch.manifest.manifest.files.validator).to_string_lossy()]
                })),
            })
            .collect())
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bootstrap-contracts")
        .join(name)
        .join("contract.yaml")
}

#[test]
fn bootstrap_manifests_are_valid_and_manifest_paths_stay_package_relative() {
    for name in ["manifest", "isolation", "batch-execution", "verification"] {
        let plan = ManifestPlan::load(fixture(name)).unwrap();
        assert_eq!(plan.manifest.version, "1");
    }

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("dataset.json"), "{}").unwrap();
    std::fs::write(
        temp.path().join("validator.py"),
        "def validate(x): return x\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("contract.yaml"),
        r#"
version: "1"
name: invalid
files: {dataset: dataset.json, validator: validator.py}
filesystem: {read: ["/etc/passwd"], write: []}
"#,
    )
    .unwrap();
    assert!(ManifestPlan::load(temp.path().join("contract.yaml")).is_err());
}

#[test]
fn isolation_staging_uses_fresh_roots_and_copies_only_declared_reads() {
    let package = tempfile::tempdir().unwrap();
    std::fs::write(
        package.path().join("dataset.json"),
        r#"{"version":1,"cases":[]}"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("undeclared-secret.txt"),
        "must not stage",
    )
    .unwrap();
    std::fs::write(
        package.path().join("contract.yaml"),
        r#"
version: "1"
name: isolation-random-root
files: {dataset: dataset.json, validator: validator.py}
filesystem: {read: [dataset.json, validator.py], write: [reports]}
"#,
    )
    .unwrap();
    let plan = ManifestPlan::load(package.path().join("contract.yaml")).unwrap();
    let first = StagedTree::create(&plan).unwrap();
    let second = StagedTree::create(&plan).unwrap();
    assert_ne!(first.root, second.root);
    assert!(first.root.join("dataset.json").is_file());
    assert!(!first.root.join("undeclared-secret.txt").exists());
    assert_eq!(Backend::Landlock.as_str(), "landlock");
    assert_eq!(Backend::Bubblewrap.as_str(), "bwrap");
}

#[test]
fn landlock_allows_manifest_reads_and_denies_unlisted_filesystem_reads() {
    let package = tempfile::tempdir().unwrap();
    std::fs::write(
        package.path().join("dataset.json"),
        r#"{"version":1,"cases":[]}"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("contract.yaml"),
        r#"
version: "1"
name: landlock-read-policy
files: {dataset: dataset.json, validator: validator.py}
filesystem: {read: [dataset.json, validator.py], write: [reports]}
"#,
    )
    .unwrap();
    let plan = ManifestPlan::load(package.path().join("contract.yaml")).unwrap();
    let tree = StagedTree::create(&plan).unwrap();
    let output = tree.run_command(
        Backend::Landlock,
        &tree.root,
        &[
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('dataset.json').is_file();\ntry: Path('/etc/passwd').read_text(); print('outside-readable')\nexcept PermissionError: print('outside-denied')".into(),
        ],
        b"",
        Some("python"),
    ).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("outside-denied"));
}

#[test]
fn batch_contract_runs_cases_steps_and_nested_contracts_in_one_tree() {
    let adapter = EchoBatch::default();
    let report = run_contract_tree(fixture("batch-execution"), Backend::None, &adapter).unwrap();
    assert_eq!(report.passed(), 3);
    assert_eq!(report.total(), 3);
    assert_eq!(
        adapter.calls.lock().unwrap().len(),
        2,
        "one adapter batch per contract package"
    );
    assert_eq!(
        adapter.invocations.load(Ordering::SeqCst),
        1,
        "one adapter coordinator runs inside the root isolation"
    );
    assert_eq!(
        report.coverage.len(),
        2,
        "root report retains parent and child native coverage"
    );
    assert!(report.coverage_complete());
    assert!(report
        .results
        .iter()
        .any(|result| result.contract == "nested-batch-bootstrap"));
    let sequence = report
        .results
        .iter()
        .find(|result| result.case_id == "ordered-steps")
        .unwrap();
    assert_eq!(sequence.steps.len(), 2);
    let root_call = adapter
        .calls
        .lock()
        .unwrap()
        .iter()
        .find(|(name, _)| name == "batch-execution-bootstrap")
        .unwrap()
        .1
        .clone();
    assert_eq!(
        root_call
            .iter()
            .filter_map(|request| request.key.step_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["step-one", "step-two"]
    );
}

#[test]
fn verifier_matches_exact_values_and_predicates_after_adapter_outputs() {
    assert_eq!(
        match_output(&json!({"value": 4}), Some(&json!({"value": 4})), None).unwrap(),
        MatchKind::Exact
    );
    assert_eq!(
        match_output(&json!({"value": 4}), None, Some(".value >= 4")).unwrap(),
        MatchKind::Predicate
    );
    assert!(match_output(&json!({"value": 3}), None, Some(".value >= 4")).is_err());
    assert!(match_output(&Value::Null, Some(&Value::Null), Some("true")).is_err());
}

#[test]
fn coverage_proof_reports_declared_source_files_missing_from_native_data() {
    let package = tempfile::tempdir().unwrap();
    std::fs::write(
        package.path().join("dataset.json"),
        r#"{"version":1,"cases":[{"id":"one","input":{"value":1},"expected":{"value":1}}]}"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(x): return x\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("support.py"),
        "def helper(x): return x\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("contract.yaml"),
        r#"
version: "1"
name: coverage-proof
files: {dataset: dataset.json, validator: validator.py, support: [support.py]}
filesystem: {read: [dataset.json, validator.py, support.py], write: [reports]}
"#,
    )
    .unwrap();
    let report = run_contract_tree(
        package.path().join("contract.yaml"),
        Backend::None,
        &EchoBatch::default(),
    )
    .unwrap();
    assert_eq!(report.passed(), 1);
    assert!(!report.coverage_complete());
    assert_eq!(report.coverage[0].missing_sources, vec!["support.py"]);
}

#[test]
fn built_in_python_and_javascript_adapters_execute_batches_and_return_coverage() {
    for name in ["manifest", "verification"] {
        let report = run_contract_tree(fixture(name), Backend::None, &RuntimeBatchAdapter).unwrap();
        assert_eq!(report.passed(), report.total(), "{name} bootstrap contract");
        let expected_cases = if name == "manifest" { 1 } else { 2 };
        assert_eq!(report.total(), expected_cases, "{name} dataset case count");
        assert_eq!(report.coverage.len(), 1);
        assert!(report.coverage_complete());
        assert!(report.coverage[0].native_report.get("format").is_some());
    }
}

#[test]
fn nested_python_and_javascript_contracts_run_under_one_landlock_boundary() {
    let report = run_contract_tree(
        fixture("batch-execution"),
        Backend::Landlock,
        &RuntimeBatchAdapter,
    )
    .unwrap();
    assert_eq!(report.passed(), report.total());
    assert_eq!(report.total(), 3);
    assert_eq!(report.coverage.len(), 2);
    assert!(report.coverage_complete());
    assert!(report
        .coverage
        .iter()
        .any(|item| item.contract == "nested-batch-bootstrap"));
}
