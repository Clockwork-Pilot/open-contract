use bootstrap_dataset_harness::DatasetCase;
use runner_batch::{
    run_contract_tree, AdapterBatch, AdapterResult, BatchAdapter, ContractBatch, CoverageFile,
    CoverageScope,
};
use runner_isolation::{Backend, StagedTree};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Validate one dataset entry by constructing the requested contract input and
/// invoking the batch runner's public contract execution API.
pub(super) fn validate_dataset_entry(entry: &DatasetCase) -> Result<(), String> {
    validate_dataset_entry_with_adapter(entry, &IdentityDatasetAdapter)
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
    if input["operation"].as_str() == Some("dataset-read-error") {
        let path = package.join("missing-dataset.json");
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
        write(package.join(support), "def support(value): return value\n")?;
        manifest["files"]["support"] = json!([support]);
        manifest["filesystem"]["read"] = json!([dataset_name, validator, support]);
    }
    #[cfg(test)]
    tests::configure_supplemental_fixture(input, package, &mut manifest)?;
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
                    "sources": [batch.manifest.manifest.files.validator],
                })),
                coverage_scopes: batch
                    .requests
                    .iter()
                    .map(|request| CoverageScope {
                        key: request.key.clone(),
                        files: vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        }],
                    })
                    .collect(),
                coverage_lines: vec![runner_batch::CoverageLineFile {
                    path: batch.manifest.manifest.files.validator.clone(),
                    executable_lines: vec![1],
                    covered_lines: vec![1],
                }],
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
