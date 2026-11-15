use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_batch::{
    run_contract_tree, show_isolation_plan, AdapterBatch, AdapterResult, BatchAdapter,
    ContractBatch, CoverageFile, CoverageScope, ExecutionKey, ManifestBatchAdapter,
};
use runner_isolation::{Backend, StagedTree};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

pub(crate) fn execute_dataset() -> Result<usize, String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(coverage)]
    let capture_path = std::env::var_os("OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS");
    #[cfg(coverage)]
    let mut snapshots = Vec::new();
    let result = run_dataset(package.join("dataset.json"), |_, entry| {
        #[cfg(coverage)]
        if capture_path.is_some() {
            unsafe { profile::reset_counters() };
        }
        validate_dataset_entry(entry)?;
        #[cfg(coverage)]
        if capture_path.is_some() {
            snapshots.push((entry.id.clone(), profile::snapshot()?));
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

/// Validate one dataset entry by constructing the requested contract input and
/// invoking the batch runner's public contract execution API.
pub(super) fn validate_dataset_entry(entry: &DatasetCase) -> Result<(), String> {
    let adapter: &dyn BatchAdapter = match entry.id.as_str() {
        id if id.starts_with("manifest-adapter-") || id.starts_with("supplemental-") => {
            &ManifestBatchAdapter
        }
        id if id.starts_with("dataset-adapter-") => {
            return validate_dataset_entry_with_adapter(
                entry,
                &ScenarioDatasetAdapter { id: id.to_owned() },
            );
        }
        _ => &IdentityDatasetAdapter,
    };
    validate_dataset_entry_with_adapter(entry, adapter)
}

fn validate_dataset_entry_with_adapter(
    entry: &DatasetCase,
    adapter: &dyn BatchAdapter,
) -> Result<(), String> {
    let id = entry.id.as_str();
    let input = &entry.input;
    let expected = entry.expected_value()?;
    let fixture = tempdir().map_err(|error| format!("fixture directory: {error}"))?;
    let package = fixture.path();
    if id == "load-dataset-reports-missing-file" {
        if !entry.setup {
            return Err("missing-file case requires setup: true".into());
        }
        let relative_path = input["path"]
            .as_str()
            .ok_or_else(|| "load_dataset input.path must be a string".to_string())?;
        let path = package.join(relative_path);
        let error = runner_batch::load_dataset(&path)
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
        if let Some(parent) = package.join(support).parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create support fixture directory: {error}"))?;
        }
        write(package.join(support), "def support(value): return value\n")?;
        manifest["files"]["support"] = json!([support]);
        manifest["filesystem"]["read"] = json!([dataset_name, validator, support]);
    }
    #[cfg(test)]
    tests::configure_supplemental_fixture(input, package, &mut manifest)?;
    if let Some(command) = input["command"].as_array() {
        manifest["execution"] = json!({"command": command});
    }
    if entry.setup {
        configure_case_setup(id, input, package, &mut manifest)?;
    }
    add_nested_contract(package, input, &mut manifest)?;
    let manifest_path = package.join("contract.yaml");
    let manifest_source = input["manifest_yaml"].as_str().map_or_else(
        || serde_yaml::to_string(&manifest).map_err(|error| error.to_string()),
        |source| Ok(source.to_owned()),
    )?;
    write(manifest_path.clone(), &manifest_source)?;

    if input["operation"].as_str() == Some("show-isolation-plan") {
        let backend = match input["backend"].as_str() {
            Some("none") => Backend::None,
            Some("bubblewrap") => Backend::Bubblewrap,
            Some("landlock") => Backend::Landlock,
            _ => return Err("show-isolation-plan requires a supported backend".into()),
        };
        let output = match show_isolation_plan(&manifest_path, backend, None) {
            Ok(output) => output,
            Err(error) => {
                if expected["error_contains"]
                    .as_str()
                    .is_some_and(|needle| error.contains(needle))
                {
                    return Ok(());
                }
                return Err(error);
            }
        };
        for expected_line in expected["contains"]
            .as_array()
            .ok_or_else(|| "expected.contains must be an array".to_string())?
        {
            let expected_line = expected_line
                .as_str()
                .ok_or_else(|| "expected.contains entries must be strings".to_string())?;
            if !output.contains(expected_line) {
                return Err(format!("expected plan to contain {expected_line:?}, got {output:?}"));
            }
        }
        return Ok(());
    }

    let result = run_contract_tree(&manifest_path, Backend::None, adapter);
    check_expected(result, expected)
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
        r#"{"version":1,"cases":[{"id":"child","input":{},"expected":{},"coverage_fingerprint":"60c6194cbdc58033999a1fb07d147da8"}]}"#,
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

fn configure_case_setup(
    id: &str,
    input: &Value,
    package: &Path,
    manifest: &mut Value,
) -> Result<(), String> {
    match id {
        "dataset-adapter-expected-error"
        | "dataset-adapter-unexpected-error"
        | "dataset-adapter-step-error"
        | "dataset-adapter-step-error-does-not-match"
        | "dataset-adapter-multiple-step-errors"
        | "dataset-adapter-no-output"
        | "dataset-adapter-omits-result"
        | "dataset-adapter-missing-coverage"
        | "dataset-adapter-duplicate-result"
        | "dataset-adapter-extra-result"
        | "dataset-adapter-missing-step-result"
        | "dataset-adapter-invalid-coverage-path"
        | "dataset-adapter-unrequested-coverage"
        | "dataset-adapter-duplicate-coverage"
        | "dataset-adapter-zero-coverage-line"
        | "dataset-adapter-empty-coverage-scope"
        | "dataset-adapter-sequence-empty-coverage"
        | "dataset-adapter-empty-coverage-files"
        | "dataset-adapter-undeclared-coverage-source"
        | "dataset-adapter-native-coverage-no-sources"
        | "dataset-adapter-native-coverage-non-string-source"
        | "dataset-adapter-multiple-coverage-lines"
        | "dataset-adapter-no-line-coverage"
        | "dataset-adapter-missing-step-coverage"
        | "dataset-adapter-missing-batch"
        | "dataset-adapter-error"
        | "dataset-adapter-wrong-expected-error"
        => Ok(()),
        "run-contract-tree-staging-rejects-symlink-escape"
        | "show-isolation-plan-staging-rejects-symlink-escape" => {
            let staged = package.join("staged");
            fs::create_dir_all(&staged)
                .map_err(|error| format!("create staged directory fixture: {error}"))?;
            #[cfg(unix)]
            std::os::unix::fs::symlink("/proc/self/status", staged.join("escape.py"))
                .map_err(|error| format!("create staging symlink fixture: {error}"))?;
            #[cfg(not(unix))]
            return Err("staging symlink dataset requires a Unix host".into());
            manifest["filesystem"]["read"] = json!(["dataset.json", "validator.py", "staged"]);
            Ok(())
        }
        "dataset-adapter-duplicate-nested-name" | "dataset-adapter-nested-contract-tree" => {
            let child = package.join("child");
            fs::create_dir_all(&child)
                .map_err(|error| format!("create nested contract fixture: {error}"))?;
            let child_dataset = serde_json::to_vec(&input["child_dataset"])
                .map_err(|error| format!("serialize nested dataset fixture: {error}"))?;
            write_bytes(&child.join("dataset.json"), &child_dataset)?;
            write(child.join("validator.py"), "def validate(value): return value\n")?;
            let child_manifest = input["child_manifest_yaml"]
                .as_str()
                .ok_or_else(|| "nested case requires child_manifest_yaml".to_string())?;
            write(child.join("contract.yaml"), child_manifest)?;
            Ok(())
        }
        id if id.starts_with("manifest-adapter-") || id.starts_with("supplemental-") => {
            let script = match id {
                "manifest-adapter-executes-batch" => r#"import json, sys
batch = json.load(sys.stdin)
requests = batch["requests"]
print(json.dumps({
    "results": [{"key": request["key"], "output": request["input"]} for request in requests],
    "coverage": {"format": "bootstrap-dataset", "sources": [batch["validator"]]},
    "coverage_scopes": [
        {"key": request["key"], "files": [{"path": batch["validator"], "lines": [1]}]}
        for request in requests
    ],
    "coverage_lines": [{"path": batch["validator"], "executable_lines": [1], "covered_lines": [1]}]
}))
"#,
                "manifest-adapter-command-fails" => "import sys\nsys.exit(1)\n",
                "manifest-adapter-returns-invalid-json" => "print('{')\n",
                "manifest-adapter-returns-no-results" => "import json\nprint(json.dumps({'results': []}))\n",
                "manifest-adapter-missing-execution" => "print('{}')\n",
                "manifest-adapter-command-cannot-start" => "print('{}')\n",
                "supplemental-coverage-is-reported" => r#"import json, sys
batch = json.load(sys.stdin)
requests = batch["requests"]
print(json.dumps({
    "results": [{"key": request["key"], "output": request["input"]} for request in requests],
    "coverage": {"format": "bootstrap-dataset", "sources": [batch["validator"]]},
    "coverage_scopes": [
        {"key": request["key"], "files": [{"path": batch["validator"], "lines": [1]}]}
        for request in requests
    ],
    "coverage_lines": [{"path": batch["validator"], "executable_lines": [1], "covered_lines": [1]}],
    "additional_test_coverage": {"executed": True, "passed": True, "native_report": {"format": "fixture"}},
    "additional_coverage_lines": [{"path": batch["validator"], "executable_lines": [1, 2], "covered_lines": [2]}]
}))
"#,
                "supplemental-tests-low-dataset-coverage" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":True}}))
"#,
                "supplemental-coverage-without-tests" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":True}}))
"#,
                "supplemental-coverage-missing-report" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}]}))
"#,
                "supplemental-tests-failed" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":False,"error":"supplemental test failure"}}))
"#,
                "supplemental-tests-skipped" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}]}))
"#,
                "supplemental-tests-no-executable-lines" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[]}))
"#,
                "supplemental-tests-coverage-without-run" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}],"additional_coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[2]}]}))
"#,
                "supplemental-tests-not-executed" => r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["validator"]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":False,"passed":True}}))
"#,
                _ => return Err(format!("no adapter fixture is defined for dataset case {id:?}")),
            };
            let adapter = "adapter.runner";
            write(package.join(adapter), script)?;
            manifest["files"]["support"] = json!([adapter]);
            manifest["filesystem"]["read"] = json!(["dataset.json", "validator.py", adapter]);
            if input["supplemental_tests"].as_bool() == Some(true) {
                manifest["filesystem"]["read"]
                    .as_array_mut()
                    .ok_or_else(|| "fixture read list must be an array".to_string())?
                    .push(json!("tests/supplemental.py"));
            }
            if id.starts_with("supplemental-tests-") || id == "supplemental-coverage-missing-report" {
                fs::create_dir_all(package.join("tests"))
                    .map_err(|error| format!("create supplemental test directory: {error}"))?;
                write(package.join("tests/supplemental.py"), "def run(): return True\n")?;
                manifest["files"]["supplemental_tests"] = json!(["tests/supplemental.py"]);
                manifest["filesystem"]["read"] = json!([
                    "dataset.json",
                    "validator.py",
                    adapter,
                    "tests/supplemental.py"
                ]);
            }
            if id == "manifest-adapter-missing-execution" {
                manifest
                    .as_object_mut()
                    .ok_or_else(|| "generated manifest is not an object".to_string())?
                    .remove("execution");
            }
            Ok(())
        }
        _ => Err(format!("no setup is defined for dataset case {id:?}")),
    }
}

fn check_expected(
    result: Result<runner_batch::BatchReport, String>,
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
    #[cfg(test)]
    tests::check_supplemental_status(&report, expected)?;
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
    Ok(())
}

/// Deterministic adapter used only to drive the bootstrap contract's public
/// dataset inputs through the batch API.
struct IdentityDatasetAdapter;

struct ScenarioDatasetAdapter {
    id: String,
}

impl BatchAdapter for ScenarioDatasetAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        let mut outputs = IdentityDatasetAdapter.execute_tree(tree, backend, batches)?;
        for batch in &mut outputs {
            for result in &mut batch.results {
                match self.id.as_str() {
                    "dataset-adapter-expected-error"
                    | "dataset-adapter-unexpected-error"
                    | "dataset-adapter-step-error"
                    | "dataset-adapter-step-error-does-not-match" => {
                        result.output = None;
                        result.error = Some("fixture failure".into());
                    }
                    "dataset-adapter-no-output" => {
                        result.output = None;
                        result.error = None;
                    }
                    _ => {}
                }
            }
            match self.id.as_str() {
                "dataset-adapter-omits-result" => batch.results.clear(),
                "dataset-adapter-missing-coverage" => batch.coverage_scopes.clear(),
                "dataset-adapter-duplicate-result" => {
                    if let Some(result) = batch.results.first().cloned() {
                        batch.results.push(result);
                    }
                }
                "dataset-adapter-extra-result" => batch.results.push(AdapterResult {
                    key: ExecutionKey {
                        target_id: "unrequested".into(),
                        case_id: "unrequested".into(),
                        step_id: None,
                    },
                    output: Some(json!({})),
                    error: None,
                }),
                "dataset-adapter-missing-step-result" => {
                    batch.results.retain(|result| result.key.step_id.is_none());
                }
                "dataset-adapter-invalid-coverage-path" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope.files.first_mut() {
                            file.path = "../outside.py".into();
                        }
                    }
                }
                "dataset-adapter-undeclared-coverage-source" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope.files.first_mut() {
                            file.path = "undeclared.py".into();
                        }
                    }
                }
                "dataset-adapter-native-coverage-no-sources" => {
                    batch.coverage = Some(json!({"format": "fixture"}));
                }
                "dataset-adapter-native-coverage-non-string-source" => {
                    batch.coverage = Some(json!({"sources": [7]}));
                }
                "dataset-adapter-multiple-coverage-lines" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope.files.iter_mut().find(|file| file.path == "support.py") {
                            file.lines = vec![1, 2];
                        }
                    }
                }
                "dataset-adapter-empty-coverage-files" => {
                    for scope in &mut batch.coverage_scopes {
                        scope.files.clear();
                    }
                }
                "dataset-adapter-unrequested-coverage" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        scope.key.target_id = "unrequested".into();
                    }
                }
                "dataset-adapter-duplicate-coverage" => {
                    if let Some(scope) = batch.coverage_scopes.first().cloned() {
                        batch.coverage_scopes.push(scope);
                    }
                }
                "dataset-adapter-zero-coverage-line" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope.files.first_mut() {
                            file.lines = vec![0];
                        }
                    }
                }
                "dataset-adapter-empty-coverage-scope" => {
                    for scope in &mut batch.coverage_scopes {
                        for file in &mut scope.files {
                            file.lines.clear();
                        }
                    }
                }
                "dataset-adapter-sequence-empty-coverage" => {
                    for scope in &mut batch.coverage_scopes {
                        for file in &mut scope.files {
                            file.lines.clear();
                        }
                    }
                }
                "dataset-adapter-no-line-coverage" => batch.coverage_lines.clear(),
                "dataset-adapter-missing-step-coverage" => {
                    batch.coverage_scopes.retain(|scope| scope.key.step_id.is_none());
                }
                "dataset-adapter-wrong-expected-error" => {
                    for result in &mut batch.results {
                        result.output = None;
                        result.error = Some("different fixture error".into());
                    }
                }
                _ => {}
            }
        }
        if self.id == "dataset-adapter-error" {
            return Err("fixture adapter failure".into());
        }
        if self.id == "dataset-adapter-missing-batch" {
            outputs.pop();
        }
        Ok(outputs)
    }
}

impl BatchAdapter for IdentityDatasetAdapter {
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
                    "format": "bootstrap-dataset",
                    "sources": std::iter::once(&batch.manifest.manifest.files.validator)
                        .chain(batch.manifest.manifest.files.support.iter()).collect::<Vec<_>>(),
                })),
                coverage_scopes: batch
                    .requests
                    .iter()
                    .map(|request| CoverageScope {
                        key: request.key.clone(),
                        files: std::iter::once(&batch.manifest.manifest.files.validator)
                            .chain(batch.manifest.manifest.files.support.iter())
                            .map(|path| CoverageFile { path: path.clone(), lines: vec![1] })
                            .collect(),
                    })
                    .collect(),
                coverage_lines: std::iter::once(&batch.manifest.manifest.files.validator)
                    .chain(batch.manifest.manifest.files.support.iter())
                    .map(|path| runner_batch::CoverageLineFile {
                        path: path.clone(), executable_lines: vec![1], covered_lines: vec![1],
                    })
                    .collect(),
                ..AdapterBatch::default()
            })
            .collect())
    }
}

fn write(path: PathBuf, contents: &str) -> Result<(), String> {
    fs::write(&path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), String> {
    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
#[path = "validator_tests.rs"]
mod tests;
