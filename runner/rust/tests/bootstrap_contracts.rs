use open_contract_core::batch::{
    AdapterBatch, AdapterResult, BatchAdapter, ContractBatch, ExecutionRequest,
};
use open_contract_core::verify::{match_output, MatchKind};
use open_contract_core::{
    run_contract_tree, Backend, ManifestBatchAdapter, ManifestPlan, StagedTree,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct EchoBatch {
    calls: Mutex<Vec<(String, Vec<ExecutionRequest>)>>,
    invocations: AtomicUsize,
}

struct SupplementalBatch {
    passed: bool,
}

impl BatchAdapter for SupplementalBatch {
    fn execute_tree(
        &self,
        _tree: &StagedTree,
        _backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
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
                    "format": "fixture",
                    "sources": [batch.manifest.manifest.files.validator.clone()]
                })),
                coverage_lines: vec![open_contract_core::batch::CoverageLineFile {
                    path: "validator.py".into(),
                    executable_lines: vec![1],
                    covered_lines: vec![1],
                }],
                additional_test_coverage: Some(json!({
                    "executed": true,
                    "passed": self.passed,
                    "error": if self.passed { Value::Null } else { json!("mock test failure") },
                    "native_report": {"format": "fixture"}
                })),
                additional_coverage_lines: vec![open_contract_core::batch::CoverageLineFile {
                    path: "validator.py".into(),
                    executable_lines: vec![1],
                    covered_lines: vec![1],
                }],
                ..AdapterBatch::default()
            })
            .collect())
    }
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
                    "sources": [batch.manifest.manifest.files.validator.clone()]
                })),
                coverage_scopes: batch
                    .requests
                    .iter()
                    .map(|request| open_contract_core::batch::CoverageScope {
                        key: request.key.clone(),
                        files: vec![open_contract_core::batch::CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        }],
                    })
                    .collect(),
                coverage_lines: std::iter::once(batch.manifest.manifest.files.validator.clone())
                    .chain(batch.manifest.manifest.files.support.iter().cloned())
                    .map(|path| open_contract_core::batch::CoverageLineFile {
                        path,
                        executable_lines: vec![1],
                        covered_lines: vec![1],
                    })
                    .collect(),
                ..AdapterBatch::default()
            })
            .collect())
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner/bootstrap-contracts")
        .join(name)
        .join("contract.yaml")
}

fn write_cross_language_fixture(package: &Path) -> PathBuf {
    let child = package.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        package.join("dataset.json"),
        r#"{"version":1,"cases":[{"id":"python-case","input":{"value":1},"expected":{"value":1}}]}"#,
    )
    .unwrap();
    std::fs::write(
        package.join("validator.py"),
        "def validate(value):\n    return value\n",
    )
    .unwrap();
    std::fs::write(
        package.join("run-contract.py"),
        "import json, os, sys\nlanguage_path = os.environ['OPEN_CONTRACT_LANGUAGE_PATH']\nsys.path.insert(0, language_path)\nfrom open_contract_language.batch_adapter import run_dataset\ndef load_validator():\n    from validator import validate\n    return validate\nreport = run_dataset(json.load(sys.stdin), validator_loader=load_validator)\nprint(json.dumps(report))\n",
    )
    .unwrap();
    std::fs::write(
        package.join("contract.yaml"),
        r#"
version: "1"
name: python-root
files:
  dataset: dataset.json
  validator: validator.py
  nested_contracts: [child/contract.yaml]
execution:
  command: [python3, run-contract.py]
language_package: python
filesystem:
  read: [dataset.json, validator.py, run-contract.py, child/contract.yaml]
  write: []
"#,
    )
    .unwrap();
    std::fs::write(
        child.join("dataset.yaml"),
        "version: 1\ncases:\n  - id: js-case\n    input: {value: 2}\n    expected: {value: 2}\n",
    )
    .unwrap();
    std::fs::write(
        child.join("validator.mjs"),
        "export function validate(value) { return value; }\n",
    )
    .unwrap();
    std::fs::write(
        child.join("run-contract.mjs"),
        "import path from 'node:path';\nimport { pathToFileURL } from 'node:url';\nconst languagePath = process.env.OPEN_CONTRACT_LANGUAGE_PATH;\nif (!languagePath) throw new Error('missing language support path');\nconst { runDataset } = await import(pathToFileURL(path.join(languagePath, 'batch-adapter.js')).href);\nlet text = '';\nfor await (const chunk of process.stdin) text += chunk;\nconst request = JSON.parse(text);\nconst report = await runDataset(request, () => import('./validator.mjs'));\nprocess.stdout.write(`${JSON.stringify(report)}\\n`);\n",
    )
    .unwrap();
    std::fs::write(
        child.join("contract.yaml"),
        r#"
version: "1"
name: javascript-child
files: {dataset: dataset.yaml, validator: validator.mjs}
execution:
  command: [node, run-contract.mjs]
language_package: javascript
filesystem: {read: [dataset.yaml, validator.mjs, run-contract.mjs], write: []}
"#,
    )
    .unwrap();
    package.join("contract.yaml")
}

#[test]
fn bootstrap_manifests_are_valid_and_manifest_paths_stay_package_relative() {
    for name in [
        "manifest",
        "isolation",
        "batch-execution",
        "verification",
        "error-handling",
        "predicates",
    ] {
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
fn verifier_matches_exact_values_and_predicates_after_adapter_outputs() {
    assert_eq!(
        match_output(&json!({"value": 4}), Some(&json!({"value": 4})), None).unwrap(),
        MatchKind::Exact
    );
    assert_eq!(
        match_output(&json!({"value": 4}), None, Some(".value >= 4")).unwrap(),
        MatchKind::Predicate
    );
    assert_eq!(
        match_output(
            &json!({"value": 4}),
            Some(&json!({"value": 4})),
            Some(".value == 4")
        )
        .unwrap(),
        MatchKind::ExpectedAndPredicate
    );
    assert!(match_output(&json!({"value": 3}), None, Some(".value >= 4")).is_err());
    assert!(match_output(
        &json!({"value": 4}),
        Some(&json!({"value": 4})),
        Some(".value == 3")
    )
    .is_err());
    assert!(match_output(&Value::Null, None, None).is_err());
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
fn manifest_commands_execute_each_contract_batch_and_return_coverage() {
    let package = tempfile::tempdir().unwrap();
    let report = run_contract_tree(
        write_cross_language_fixture(package.path()),
        Backend::None,
        &ManifestBatchAdapter,
    )
    .unwrap();
    assert_eq!(report.passed(), report.total());
    assert_eq!(report.coverage.len(), 2);
    assert!(
        report.coverage_complete(),
        "coverage: {:#?}",
        report.coverage
    );
    assert!(report
        .coverage
        .iter()
        .all(|item| item.native_report.get("format").is_some()));
}

#[test]
fn cli_writes_optional_file_and_line_coverage_report_for_contract_users() {
    let output_dir = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    let report_path = output_dir.path().join("nested/coverage.json");
    let result = Command::new(env!("CARGO_BIN_EXE_open-contract-core"))
        .arg("run-manifest")
        .arg(write_cross_language_fixture(package.path()))
        .args(["--isolation", "none", "--coverage-report"])
        .arg(&report_path)
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Cases:"));
    assert!(stdout.contains("Coverage report:"));
    assert!(!stdout.trim_start().starts_with('{'));
    let report: Value = serde_json::from_slice(&std::fs::read(report_path).unwrap()).unwrap();
    assert_eq!(report["coverage_complete"], true);
    let contracts = report["contracts"].as_array().unwrap();
    assert_eq!(contracts.len(), 2);
    assert!(contracts.iter().all(|contract| {
        contract["native_report"].is_object()
            && contract["declared_sources"].is_array()
            && contract["covered_sources"].is_array()
            && contract["missing_sources"] == json!([])
            && contract["dataset_coverage"]["files"].is_array()
            && contract["dataset_coverage"]["cases"].is_array()
            && contract["additional_test_coverage"].is_null()
    }));
    let root = contracts
        .iter()
        .find(|contract| contract["contract"] == "python-root")
        .unwrap();
    assert!(root["dataset_coverage"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "validator.py"
            && file["executable_lines"]
                .as_array()
                .is_some_and(|lines| !lines.is_empty())
            && file["uncovered_lines"].is_array()));
    assert!(root["dataset_coverage"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|case| case["case_id"] == "python-case"
            && case["coverage"]["files"][0]["lines"].as_array().is_some()));
    let child = contracts
        .iter()
        .find(|contract| contract["contract"] == "javascript-child")
        .unwrap();
    assert!(child["dataset_coverage"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "child/validator.mjs"));
}

#[test]
fn cli_saves_report_and_returns_nonzero_when_manifest_coverage_threshold_is_missed() {
    let package = tempfile::tempdir().unwrap();
    let manifest = write_cross_language_fixture(package.path());
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(value):\n    if value.get('flag'):\n        result = {'value': value['value']}\n    else:\n        result = {'value': value['value']}\n    return result\n",
    )
    .unwrap();
    let source = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        source.replace(
            "name: python-root\n",
            "name: python-root\ncoverage: {minimum_line_percent: 100}\n",
        ),
    )
    .unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_open-contract-core"))
        .arg("run-manifest")
        .arg(&manifest)
        .args(["--isolation", "none"])
        .current_dir(package.path())
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stdout.contains("Cases: 2/2 passed"));
    assert!(stdout.contains("dataset 80.0%"));
    assert!(stdout.contains("uncovered lines 3"));
    assert!(stdout.contains("Coverage report: .open-contract/coverage/latest.json"));
    assert!(stderr.contains("coverage requirements not met"));
    let report: Value = serde_json::from_slice(
        &std::fs::read(package.path().join(".open-contract/coverage/latest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["coverage_complete"], false);
    assert_eq!(report["contracts"][0]["minimum_line_percent"], json!(100.0));
}

#[test]
fn manifest_supplemental_coverage_is_separate_and_fails_the_coverage_gate() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir(package.path().join("tests")).unwrap();
    std::fs::create_dir_all(package.path().join("child/tests")).unwrap();
    std::fs::write(
        package.path().join("dataset.json"),
        r#"{"version":1,"cases":[{"id":"one","input":{"value":1},"expected":{"value":1}}]}"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("tests/supplemental.py"),
        "def run(): return True\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("child/dataset.json"),
        r#"{"version":1,"cases":[{"id":"child-one","input":{"value":2},"expected":{"value":2}}]}"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("child/validator.py"),
        "def validate(value): return value\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("child/tests/supplemental.py"),
        "def run(): return True\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("child/contract.yaml"),
        "version: '1'\nname: child-supplemental-fixture\nfiles: {dataset: dataset.json, validator: validator.py, supplemental_tests: [tests/supplemental.py]}\nfilesystem: {read: [dataset.json, validator.py, tests/supplemental.py], write: []}\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("contract.yaml"),
        r#"
version: "1"
name: supplemental-coverage-fixture
files:
  dataset: dataset.json
  validator: validator.py
  supplemental_tests: [tests/supplemental.py]
  nested_contracts: [child/contract.yaml]
filesystem:
  read: [dataset.json, validator.py, tests/supplemental.py, child/contract.yaml]
  write: [reports]
"#,
    )
    .unwrap();

    let passed_report = run_contract_tree(
        package.path().join("contract.yaml"),
        Backend::None,
        &SupplementalBatch { passed: true },
    )
    .unwrap();
    assert!(passed_report.coverage_complete());
    assert_eq!(
        passed_report.coverage[0].additional_test_coverage["passed"],
        true
    );
    assert_eq!(
        passed_report.coverage[0].line_coverage[0].covered_lines,
        vec![1]
    );
    assert_eq!(
        passed_report.coverage[0].additional_line_coverage[0].covered_lines,
        vec![1]
    );
    let child = passed_report
        .coverage
        .iter()
        .find(|record| record.contract == "child-supplemental-fixture")
        .unwrap();
    assert_eq!(child.line_coverage[0].path, "child/validator.py");
    assert_eq!(child.additional_line_coverage[0].path, "child/validator.py");

    let failed_report = run_contract_tree(
        package.path().join("contract.yaml"),
        Backend::None,
        &SupplementalBatch { passed: false },
    )
    .unwrap();
    assert!(!failed_report.coverage_complete());
    assert_eq!(
        failed_report.coverage[0].additional_test_coverage["passed"],
        false
    );
}

#[test]
fn dataset_coverage_fingerprints_match_cases_and_steps() {
    let package = tempfile::tempdir().unwrap();
    std::fs::write(
        package.path().join("validator.py"),
        "def validate(x): return x\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("dataset.json"),
        r#"{
          "version": 1,
          "cases": [
            {"id":"case","input":{"value":1},"expected":{"value":1},"coverage_fingerprint":"60c6194cbdc58033999a1fb07d147da8"},
            {"id":"sequence","input":{},"steps":[
              {"id":"step","input":{"value":2},"expected":{"value":2},"coverage_fingerprint":"60c6194cbdc58033999a1fb07d147da8"}
            ]}
          ]
        }"#,
    )
    .unwrap();
    std::fs::write(
        package.path().join("contract.yaml"),
        "version: '1'\nname: fingerprint-fixture\nfiles: {dataset: dataset.json, validator: validator.py}\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
    )
    .unwrap();

    let report = run_contract_tree(
        package.path().join("contract.yaml"),
        Backend::None,
        &EchoBatch::default(),
    )
    .unwrap();
    assert_eq!(report.passed(), 2);
    assert_eq!(
        report.results[0]
            .coverage
            .as_ref()
            .unwrap()
            .fingerprint
            .len(),
        32
    );
    assert_eq!(
        report.results[0]
            .coverage
            .as_ref()
            .unwrap()
            .baseline_matches,
        Some(true)
    );
    assert_eq!(
        report.results[1].steps[0]
            .coverage
            .as_ref()
            .unwrap()
            .baseline_matches,
        Some(true)
    );

    let dataset_path = package.path().join("dataset.json");
    let mismatched = std::fs::read_to_string(&dataset_path).unwrap().replace(
        "60c6194cbdc58033999a1fb07d147da8",
        "60c6194cbdc58033999a1fb07d147da9",
    );
    std::fs::write(dataset_path, mismatched).unwrap();
    let report = run_contract_tree(
        package.path().join("contract.yaml"),
        Backend::None,
        &EchoBatch::default(),
    )
    .unwrap();
    assert_eq!(report.passed(), 0);
    assert!(report.results.iter().all(|result| result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("coverage fingerprint mismatch"))));
    assert!(report.results[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("fingerprint-fixture/default/case")));
    assert!(report.results[1].steps[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("fingerprint-fixture/default/sequence/step")));
}

#[test]
fn parent_and_nested_contracts_load_json_and_yaml_datasets_in_one_tree() {
    let package = tempfile::tempdir().unwrap();
    let report = run_contract_tree(
        write_cross_language_fixture(package.path()),
        Backend::None,
        &EchoBatch::default(),
    )
    .unwrap();
    assert_eq!(report.passed(), report.total());
    assert_eq!(report.total(), 2);
    assert_eq!(report.coverage.len(), 2);
    assert!(report.coverage_complete());
    assert!(report
        .coverage
        .iter()
        .any(|item| item.contract == "javascript-child"));
}
