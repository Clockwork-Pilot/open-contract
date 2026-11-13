use open_contract_batch::{
    run_contract_tree, AdapterBatch, AdapterResult, BatchAdapter, ContractBatch, CoverageFile,
    CoverageScope, ExecutionKey, ManifestBatchAdapter,
};
use open_contract_isolation::{Backend, StagedTree};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tempfile::tempdir;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("batch-execution dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bytes = fs::read(package.join("dataset.json"))
        .map_err(|error| format!("cannot read dataset: {error}"))?;
    let dataset: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid dataset JSON: {error}"))?;
    let cases = dataset["cases"]
        .as_array()
        .ok_or_else(|| "dataset must contain a cases array".to_string())?;
    if cases.is_empty() {
        return Err("dataset must exercise batch execution".into());
    }
    for case in cases {
        let id = case["id"].as_str().map_or("unknown", |value| value);
        run_case(id, &case["input"], &case["expected"])
            .map_err(|error| format!("cases/{id}: {error}"))?;
    }
    println!("Batch execution dataset: {} cases passed", cases.len());
    Ok(())
}

fn run_case(id: &str, input: &Value, expected: &Value) -> Result<(), String> {
    let fixture = tempdir().map_err(|error| format!("fixture directory: {error}"))?;
    let package = fixture.path();
    if input["operation"].as_str() == Some("dataset-read-error") {
        let path = package.join("missing-dataset.json");
        let error = open_contract_batch::load_dataset(&path)
            .err()
            .ok_or_else(|| "expected the missing dataset read to fail".to_string())?;
        let needle = expected["error_contains"]
            .as_str()
            .ok_or_else(|| "expected.error_contains must be a string".to_string())?;
        return if error.contains(needle) {
            Ok(())
        } else {
            Err(format!(
                "expected error containing {needle:?}, got {error:?}"
            ))
        };
    }
    let extension = input["extension"].as_str().map_or("json", |value| value);
    let dataset_name = format!("dataset.{extension}");
    let dataset_path = package.join(&dataset_name);
    let validator = input["validator_name"]
        .as_str()
        .map_or("validator.py", |value| value);
    if Path::new(validator).components().count() != 1 || validator.contains('\\') {
        return Err("validator_name must be one package-relative path component".into());
    }
    write(
        package.join(validator),
        "def validate(value): return value\n",
    )?;
    let dataset_bytes = match input["dataset_yaml"].as_str() {
        Some(source) => source.as_bytes().to_vec(),
        None if input.get("dataset").is_some() => serde_json::to_vec(&input["dataset"])
            .map_err(|error| format!("cannot serialize fixture dataset: {error}"))?,
        None => serde_json::to_vec(&json!({
            "version": 1,
            "cases": [{"id":"case", "input":{"value":1}, "expected":{"value":1}}]
        }))
        .map_err(|error| format!("cannot serialize default dataset: {error}"))?,
    };
    write_bytes(&dataset_path, &dataset_bytes)?;

    let name = format!("batch-case-{id}");
    let mut manifest = json!({
        "version": "1",
        "name": name,
        "files": {"dataset": dataset_name, "validator": validator},
        "filesystem": {"read": [dataset_name, validator], "write": []}
    });
    if input["support"].as_bool() == Some(true) || input["support_path"].is_string() {
        let support = input["support_path"]
            .as_str()
            .map_or("support.py", |value| value);
        write(package.join(support), "def support(value): return value\n")?;
        manifest["files"]["support"] = json!([support]);
        manifest["filesystem"]["read"] = json!([dataset_name, validator, support]);
    }
    if input["supplemental_tests"].as_bool() == Some(true) {
        fs::create_dir_all(package.join("tests"))
            .map_err(|error| format!("create supplemental test directory: {error}"))?;
        write(
            package.join("tests/supplemental.py"),
            "def run(): return True\n",
        )?;
        manifest["files"]["supplemental_tests"] = json!(["tests/supplemental.py"]);
        manifest["filesystem"]["read"]
            .as_array_mut()
            .ok_or_else(|| "fixture read list must be an array".to_string())?
            .push(json!("tests/supplemental.py"));
    }
    if let Some(command) = input["command"].as_array() {
        manifest["execution"] = json!({"command": command});
    }
    add_nested_contract(package, input, &mut manifest)?;
    let manifest_path = package.join("contract.yaml");
    let manifest_source = input["manifest_yaml"].as_str().map_or_else(
        || serde_yaml::to_string(&manifest).map_err(|error| error.to_string()),
        |source| Ok(source.to_owned()),
    )?;
    write(manifest_path.clone(), &manifest_source)?;

    let mode = input["adapter_mode"]
        .as_str()
        .map_or("normal", |value| value)
        .to_owned();
    let result = if input["adapter"].as_str() == Some("manifest") {
        run_contract_tree(&manifest_path, Backend::None, &ManifestBatchAdapter)
    } else {
        run_contract_tree(&manifest_path, Backend::None, &FixtureAdapter { mode })
    };
    check_expected(id, result, expected)
}

fn add_nested_contract(package: &Path, input: &Value, manifest: &mut Value) -> Result<(), String> {
    let duplicate_name = input["duplicate_nested_name"].as_bool() == Some(true);
    let cycle = input["nested_cycle"].as_bool() == Some(true);
    let nested_contract = input["nested_contract"].as_bool() == Some(true);
    if !duplicate_name && !cycle && !nested_contract {
        return Ok(());
    }
    let child = package.join("child");
    fs::create_dir_all(&child).map_err(|error| format!("create nested fixture: {error}"))?;
    write(
        child.join("dataset.json"),
        r#"{"version":1,"cases":[{"id":"child","input":{},"expected":{}}]}"#,
    )?;
    write(
        child.join("validator.py"),
        "def validate(value): return value\n",
    )?;
    let name = if duplicate_name {
        manifest["name"].as_str().map_or("duplicate", |value| value)
    } else {
        "nested-child"
    };
    let nested = if cycle {
        "  nested_contracts: [contract.yaml]\n"
    } else {
        ""
    };
    let child_manifest = format!(
        "version: '1'\nname: {name}\nfiles:\n  dataset: dataset.json\n  validator: validator.py\n{nested}filesystem:\n  read: [dataset.json, validator.py{}]\n  write: []\n",
        if cycle { ", contract.yaml" } else { "" }
    );
    write(child.join("contract.yaml"), &child_manifest)?;
    manifest["files"]["nested_contracts"] = json!(["child/contract.yaml"]);
    let reads = if input["support"].as_bool() == Some(true) || input["support_path"].is_string() {
        let support = input["support_path"]
            .as_str()
            .map_or("support.py", |value| value);
        json!([
            "dataset.json",
            "validator.py",
            support,
            "child/contract.yaml"
        ])
    } else {
        json!(["dataset.json", "validator.py", "child/contract.yaml"])
    };
    manifest["filesystem"]["read"] = reads;
    Ok(())
}

fn check_expected(
    id: &str,
    result: Result<open_contract_batch::BatchReport, String>,
    expected: &Value,
) -> Result<(), String> {
    if let Some(needle) = expected["error_contains"].as_str() {
        let observed = match result {
            Err(error) => error,
            Ok(report) => report
                .results
                .iter()
                .find_map(|result| result.error.clone())
                .ok_or_else(|| "expected an error, but the batch run succeeded".to_string())?,
        };
        return if observed.contains(needle) {
            Ok(())
        } else {
            Err(format!(
                "expected error containing {needle:?}, got {observed:?}"
            ))
        };
    }
    let report = result?;
    let expected_passed = expected["passed"]
        .as_bool()
        .ok_or_else(|| "expected.passed must be a boolean".to_string())?;
    let passed = report.passed() == report.total();
    if passed != expected_passed {
        return Err(format!(
            "expected passed={expected_passed}, got {}/{} cases passed",
            report.passed(),
            report.total()
        ));
    }
    if let Some(total) = expected["total"].as_u64() {
        if report.total() != total as usize {
            return Err(format!(
                "expected {} results, got {}",
                total,
                report.total()
            ));
        }
    }
    if let Some(coverage_complete) = expected["coverage_complete"].as_bool() {
        if report.coverage_complete() != coverage_complete {
            return Err(format!(
                "expected coverage_complete={coverage_complete}, got {}",
                report.coverage_complete()
            ));
        }
    }
    if let Some(status) = expected["supplemental_status"].as_str() {
        let observed = report
            .coverage
            .first()
            .and_then(|record| record.additional_test_coverage["status"].as_str())
            .ok_or_else(|| "supplemental coverage status was missing".to_string())?;
        if observed != status {
            return Err(format!(
                "expected supplemental status {status:?}, got {observed:?}"
            ));
        }
    }
    if let Some(needle) = expected["result_error_contains"].as_str() {
        let observed = report
            .results
            .iter()
            .find_map(|result| result.error.as_deref())
            .ok_or_else(|| "expected a result error, but none was returned".to_string())?;
        if !observed.contains(needle) {
            return Err(format!(
                "expected result error containing {needle:?}, got {observed:?}"
            ));
        }
    }
    let _ = id;
    Ok(())
}

struct FixtureAdapter {
    mode: String,
}

impl BatchAdapter for FixtureAdapter {
    fn execute_tree(
        &self,
        _tree: &StagedTree,
        _backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        if self.mode == "adapter-error" {
            return Err("fixture adapter failed".into());
        }
        if self.mode == "no-batches" {
            return Ok(Vec::new());
        }
        Ok(batches
            .iter()
            .map(|batch| AdapterBatch {
                results: self.results(batch),
                coverage: if self.mode == "coverage-without-sources" {
                    Some(json!({"format": "fixture"}))
                } else {
                    Some(json!({
                        "format": "fixture",
                        "sources": std::iter::once(&batch.manifest.manifest.files.validator)
                            .chain(&batch.manifest.manifest.files.support)
                            .cloned()
                            .collect::<Vec<_>>(),
                    }))
                },
                coverage_scopes: self.coverage(batch),
                coverage_lines: self.line_coverage(batch),
                additional_test_coverage: self.supplemental_report(),
                additional_coverage_lines: self.additional_line_coverage(batch),
                ..AdapterBatch::default()
            })
            .collect())
    }
}

impl FixtureAdapter {
    fn line_coverage(&self, batch: &ContractBatch) -> Vec<open_contract_batch::CoverageLineFile> {
        if self.mode == "supplemental-no-lines" || self.mode == "no-coverage" {
            return Vec::new();
        }
        let (executable_lines, covered_lines) = if matches!(
            self.mode.as_str(),
            "supplemental-under-threshold"
                | "supplemental-under-threshold-ran"
                | "supplemental-under-threshold-coverage"
        ) {
            (vec![1, 2], vec![1])
        } else {
            (vec![1], vec![1])
        };
        vec![open_contract_batch::CoverageLineFile {
            path: batch.manifest.manifest.files.validator.clone(),
            executable_lines,
            covered_lines,
        }]
    }

    fn additional_line_coverage(
        &self,
        batch: &ContractBatch,
    ) -> Vec<open_contract_batch::CoverageLineFile> {
        if matches!(
            self.mode.as_str(),
            "supplemental-no-lines"
                | "supplemental-under-threshold"
                | "supplemental-under-threshold-ran"
        ) || (!self.mode.starts_with("supplemental-") && self.mode != "unexpected-supplemental")
        {
            return Vec::new();
        }
        vec![open_contract_batch::CoverageLineFile {
            path: batch.manifest.manifest.files.validator.clone(),
            executable_lines: vec![1, 2],
            covered_lines: vec![2],
        }]
    }

    fn supplemental_report(&self) -> Option<Value> {
        match self.mode.as_str() {
            "unexpected-supplemental"
            | "supplemental-under-threshold"
            | "supplemental-under-threshold-coverage" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-under-threshold-ran" => Some(json!({
                "executed": true,
                "passed": true,
            })),
            "supplemental-no-lines" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-omitted" => None,
            "supplemental-not-executed" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-passed" | "supplemental-failed" => Some(json!({
                "executed": true,
                "passed": self.mode == "supplemental-passed",
                "error": if self.mode == "supplemental-failed" { "fixture supplemental failure" } else { "" },
                "native_report": {"format": "fixture"},
            })),
            _ => None,
        }
    }

    fn results(&self, batch: &ContractBatch) -> Vec<AdapterResult> {
        let mut results = batch
            .requests
            .iter()
            .map(|request| AdapterResult {
                key: request.key.clone(),
                output: (!matches!(self.mode.as_str(), "no-output" | "case-error"))
                    .then(|| request.input.clone()),
                error: (self.mode == "case-error").then(|| "fixture error".into()),
            })
            .collect::<Vec<_>>();
        if self.mode == "omit-step-results" {
            results.retain(|result| result.key.step_id.is_none());
        }
        match self.mode.as_str() {
            "no-results" => results.clear(),
            "duplicate-results" => {
                if let Some(result) = results.first().cloned() {
                    results.push(result);
                }
            }
            "extra-results" => results.push(AdapterResult {
                key: ExecutionKey {
                    target_id: "unexpected".into(),
                    case_id: "unexpected".into(),
                    step_id: None,
                },
                output: Some(Value::Null),
                error: None,
            }),
            _ => {}
        }
        results
    }

    fn coverage(&self, batch: &ContractBatch) -> Vec<CoverageScope> {
        if self.mode == "no-coverage" {
            return Vec::new();
        }
        let mut scopes = batch
            .requests
            .iter()
            .map(|request| {
                let key = if self.mode == "unrequested-coverage" {
                    ExecutionKey {
                        target_id: "unexpected".into(),
                        case_id: "unexpected".into(),
                        step_id: None,
                    }
                } else {
                    request.key.clone()
                };
                let files =
                    match self.mode.as_str() {
                        "undeclared-coverage" => vec![CoverageFile {
                            path: "outside.py".into(),
                            lines: vec![1],
                        }],
                        "invalid-coverage-path" => vec![CoverageFile {
                            path: "../escape.py".into(),
                            lines: vec![1],
                        }],
                        "zero-coverage-line" => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![0],
                        }],
                        "empty-coverage" => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: Vec::new(),
                        }],
                        "multi-file-coverage" => std::iter::once(CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        })
                        .chain(batch.manifest.manifest.files.support.iter().map(|path| {
                            CoverageFile {
                                path: path.clone(),
                                lines: vec![1, 2],
                            }
                        }))
                        .collect(),
                        _ => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        }],
                    };
                CoverageScope { key, files }
            })
            .collect::<Vec<_>>();
        if self.mode == "duplicate-coverage" {
            if let Some(scope) = scopes.first().cloned() {
                scopes.push(scope);
            }
        }
        scopes
    }
}

fn write(path: PathBuf, contents: &str) -> Result<(), String> {
    fs::write(&path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), String> {
    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}
