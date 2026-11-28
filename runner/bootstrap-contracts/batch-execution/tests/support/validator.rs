use bootstrap_dataset_harness::{run_dataset, DatasetCase};
use runner_batch::{
    load_dataset_with_schema, run_contract_tree, run_contract_tree_with_fingerprint_mode,
    run_contract_tree_with_options, show_isolation_plan, AdapterBatch, AdapterResult, BatchAdapter,
    ContractBatch, CoverageFile, CoverageScope, ExecutionKey, FingerprintMode,
    ManifestBatchAdapter,
};
use runner_isolation::{Backend, StagedTree};
use runner_manifest::{DatasetDeclaration, FileSystem, Manifest, ManifestPlan};
use runner_verification::{match_output, MatchKind};
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
    let input = &entry.input;
    if entry.id.starts_with("value-schema-") {
        return validate_value_schema_case(entry);
    }
    let adapter: &dyn BatchAdapter = match entry.id.as_str() {
        id if id.starts_with("manifest-adapter-") || id.starts_with("supplemental-") => {
            &ManifestBatchAdapter
        }
        id if id.starts_with("dataset-adapter-") => {
            let coverage_metrics = input["coverage_metrics"]
                .as_object()
                .map(|_| serde_json::from_value(input["coverage_metrics"].clone()))
                .transpose()
                .map_err(|error| format!("invalid fixture coverage_metrics: {error}"))?
                .unwrap_or_default();
            return validate_dataset_entry_with_adapter(
                entry,
                &ScenarioDatasetAdapter {
                    id: id.to_owned(),
                    coverage_metrics,
                    coverage_entities: input["coverage_entities"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                },
            );
        }
        _ => &IdentityDatasetAdapter,
    };
    validate_dataset_entry_with_adapter(entry, adapter)
}

fn validate_value_schema_case(entry: &DatasetCase) -> Result<(), String> {
    let input = &entry.input;
    let directory = tempdir().map_err(|error| format!("schema fixture directory: {error}"))?;
    let dataset_path = directory.path().join("dataset.yaml");
    let schema_path = directory.path().join("schema.yaml");
    write(
        dataset_path.clone(),
        input["dataset"]
            .as_str()
            .ok_or("schema fixture requires dataset YAML")?,
    )?;
    if input["schema_missing"].as_bool() != Some(true) {
        write(
            schema_path.clone(),
            input["schema"]
                .as_str()
                .ok_or("schema fixture requires schema YAML")?,
        )?;
    }
    let result = load_dataset_with_schema(&dataset_path, Some(&schema_path));
    let actual = match result {
        Ok(_) => json!({"valid": true}),
        Err(error) => json!({"valid": false, "error": error}),
    };
    check_public_output(&actual, entry.expected_value()?, entry.predicate.as_deref())
}

fn validate_dataset_entry_with_adapter(
    entry: &DatasetCase,
    adapter: &dyn BatchAdapter,
) -> Result<(), String> {
    let id = entry.id.as_str();
    let input = &entry.input;
    let expected = entry.expected_value()?;
    if id.starts_with("adapter-coverage-") {
        let actual = match runner_batch::validate_adapter_coverage_payload(input, "schema-fixture")
        {
            Ok(()) => json!({"valid": true}),
            Err(error) => json!({"valid": false, "error": error}),
        };
        return check_public_output(&actual, expected, entry.predicate.as_deref());
    }
    if input["operation"] == "validate-runner-manifest-filename" {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| "validate-runner-manifest-filename requires input.path".to_string())?;
        return check_public_output(
            &json!({"valid": runner_batch::has_open_contract_filename(Path::new(path))}),
            expected,
            entry.predicate.as_deref(),
        );
    }
    if input["operation"] == "validate-nested-manifest-filenames" {
        return validate_nested_manifest_filename_fixture(
            input,
            expected,
            entry.predicate.as_deref(),
        );
    }
    let fixture = tempdir().map_err(|error| format!("fixture directory: {error}"))?;
    let package = fixture.path();
    if id == "load-dataset-reports-missing-file" {
        if entry.setup.as_deref() != Some(id) {
            return Err("missing-file case requires its fixture ID in setup".into());
        }
        let relative_path = input["path"]
            .as_str()
            .ok_or_else(|| "load_dataset input.path must be a string".to_string())?;
        let path = package.join(relative_path);
        let actual = match runner_batch::load_dataset(&path) {
            Ok(dataset) => serde_json::to_value(dataset)
                .map_err(|error| format!("serialize loaded dataset: {error}"))?,
            Err(error) => json!({"error": error}),
        };
        return check_public_output(&actual, expected, entry.predicate.as_deref());
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
            "cases": [{"id":"case", "input":{"value":1}, "expected":{"value":1}, "coverage_fingerprint":"880ae786fa1c0630fe9158b7962fd8a2"}]
        }))
        .map_err(|error| format!("cannot serialize default dataset: {error}"))?,
    };
    write_bytes(&dataset_path, &dataset_bytes)?;

    let second_dataset_name = "second-dataset.json";
    let has_second_dataset = input.get("second_dataset").is_some();
    if let Some(second_dataset) = input.get("second_dataset") {
        write_bytes(
            &package.join(second_dataset_name),
            &serde_json::to_vec(second_dataset).map_err(|error| error.to_string())?,
        )?;
    }

    let name = format!("batch-case-{id}");
    let mut manifest = json!({
        "version": "1",
        "name": name,
        "datasets": if has_second_dataset { vec![dataset_name.clone(), second_dataset_name.into()] } else { vec![dataset_name.clone()] },
        "coverage_sources": [validator],
        "filesystem": {"read": [dataset_name, validator], "write": []}
    });
    if has_second_dataset {
        manifest["filesystem"]["read"]
            .as_array_mut()
            .unwrap()
            .push(json!(second_dataset_name));
    }
    if input["value_schema"].as_bool() == Some(true) {
        fs::create_dir_all(package.join("schemas"))
            .map_err(|error| format!("create schema fixture directory: {error}"))?;
        write(
            package.join("schemas/object-values.yaml"),
            "input: {type: object, required: [value], fields: {value: integer}}\nexpected: {type: object, required: [value], fields: {value: integer}}\n",
        )?;
        write(
            package.join("schemas/integer-values.yaml"),
            "input: integer\nexpected: integer\n",
        )?;
        manifest["datasets"] = json!([
            {"path": dataset_name, "schema": "schemas/object-values.yaml"},
            {"path": second_dataset_name, "schema": "schemas/integer-values.yaml"}
        ]);
        manifest["filesystem"]["read"] = json!([
            dataset_name,
            second_dataset_name,
            validator,
            "schemas/object-values.yaml",
            "schemas/integer-values.yaml"
        ]);
        if input["invalid_value_schema"].as_bool() == Some(true) {
            write(
                package.join("schemas/object-values.yaml"),
                "input: string\nexpected: {type: object, fields: {value: integer}}\n",
            )?;
        }
    }
    if let Some(minimum_coverage_percent) = input.get("minimum_coverage_percent") {
        manifest["minimum_coverage_percent"] = minimum_coverage_percent.clone();
    }
    if let Some(external_read_path) = input["external_read_path"].as_str() {
        manifest["filesystem"]["read"]
            .as_array_mut()
            .ok_or_else(|| "fixture read list must be an array".to_string())?
            .push(json!(external_read_path));
    }
    if let Some(duplicate_read_path) = input["duplicate_read_path"].as_str() {
        manifest["filesystem"]["read"]
            .as_array_mut()
            .ok_or_else(|| "fixture read list must be an array".to_string())?
            .push(json!(duplicate_read_path));
    }
    if input["support"].as_bool() == Some(true) || input["support_path"].is_string() {
        let support = input["support_path"]
            .as_str()
            .map_or("support.py", |value| value);
        if let Some(parent) = package.join(support).parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create support fixture directory: {error}"))?;
        }
        write(
            package.join(support),
            "def support(value): return value\n# second source line\n",
        )?;
        manifest["coverage_sources"]
            .as_array_mut()
            .unwrap()
            .push(json!(support));
        manifest["filesystem"]["read"] = json!([dataset_name, validator, support]);
    }
    #[cfg(test)]
    tests::configure_supplemental_fixture(input, package, &mut manifest)?;
    if let Some(command) = input["command"].as_array() {
        manifest["execution"] = json!({"command": command});
    }
    if let Some(setup_id) = entry.setup.as_deref() {
        configure_case_setup(setup_id, input, package, &mut manifest)?;
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
            Some("tempdir") => Backend::TempDir,
            Some("none") => Backend::None,
            Some("bubblewrap") => Backend::Bubblewrap,
            Some("landlock") => Backend::Landlock,
            _ => return Err("show-isolation-plan requires a supported backend".into()),
        };
        let isolation_runtime = input["isolation_runtime"].as_str().map(PathBuf::from);
        let actual = match show_isolation_plan(&manifest_path, backend, isolation_runtime) {
            Ok(output) => json!({"plan": output}),
            Err(error) => json!({"error": error}),
        };
        return check_public_output(&actual, expected, entry.predicate.as_deref());
    }

    let backend = match input["backend"].as_str() {
        None | Some("none") => Backend::None,
        Some("tempdir") => Backend::TempDir,
        Some("bubblewrap") => Backend::Bubblewrap,
        Some("landlock") => Backend::Landlock,
        Some(_) => return Err("run-contract-tree requires a supported backend".into()),
    };
    let result = if input["fingerprint_mode"].as_str() == Some("update") {
        run_contract_tree_with_fingerprint_mode(
            &manifest_path,
            backend,
            None,
            FingerprintMode::Update,
            adapter,
        )
    } else if input["coverage_fingerprints"].as_bool() == Some(false) {
        run_contract_tree_with_options(&manifest_path, backend, None, false, adapter)
    } else if let Some(include_nested) = input["include_nested_contracts"].as_bool() {
        runner_batch::run_contract_tree_with_fingerprint_mode_and_nested_contracts(
            &manifest_path,
            backend,
            None,
            FingerprintMode::Disabled,
            include_nested,
            adapter,
        )
    } else {
        run_contract_tree(&manifest_path, backend, adapter)
    };
    check_expected(result, expected, entry.predicate.as_deref())
}

fn validate_nested_manifest_filename_fixture(
    input: &Value,
    expected: &Value,
    predicate: Option<&str>,
) -> Result<(), String> {
    let package = tempdir().map_err(|error| format!("nested manifest fixture: {error}"))?;
    let nested_paths = input["nested_contracts"]
        .as_array()
        .ok_or_else(|| "nested filename fixture requires nested_contracts".to_string())?
        .iter()
        .map(|path| {
            path.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "nested_contracts items must be strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(path) = nested_paths.first() {
        if path.starts_with("../") {
            // Deliberately leave the path unresolved to exercise the runner's
            // error propagation for an invalid nested reference.
        } else {
            let child_path = package.path().join(path);
            if let Some(parent) = child_path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(child_path.parent().unwrap().join("dataset.json"), "{}")
                .map_err(|error| error.to_string())?;
            fs::write(
                child_path.parent().unwrap().join("validator.py"),
                "def validate(value): return value\n",
            )
            .map_err(|error| error.to_string())?;
            let contents = input["child_manifest"]
                .as_str()
                .unwrap_or("version: '1'\nname: child\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n");
            fs::write(child_path, contents).map_err(|error| error.to_string())?;
        }
    }
    let plan = ManifestPlan {
        manifest: Manifest {
            version: "1".into(),
            name: "fixture-root".into(),
            datasets: vec![DatasetDeclaration::Path("dataset.json".into())],
            coverage_sources: vec!["validator.py".into()],
            metadata: None,
            supplemental_tests: Vec::new(),
            nested_contracts: nested_paths,
            directories: Default::default(),
            minimum_coverage_percent: 100.0,
            execution: None,
            language_adapter: None,
            filesystem: FileSystem {
                read: Vec::new(),
                write: Vec::new(),
            },
        },
        manifest_path: package.path().join("root.open-contract.yaml"),
        package_root: package.path().to_path_buf(),
        language_adapter: None,
    };
    let actual = match runner_batch::validate_nested_manifest_filenames(&plan) {
        Ok(()) => json!({"valid": true}),
        Err(error) => json!({"valid": false, "error": error}),
    };
    check_public_output(&actual, expected, predicate)
}

fn add_nested_contract(package: &Path, input: &Value, manifest: &mut Value) -> Result<(), String> {
    let duplicate_name = input["duplicate_nested_name"].as_bool() == Some(true);
    let cycle = input["nested_cycle"].as_bool() == Some(true);
    let nested_contract = input["nested_contract"].as_bool() == Some(true);
    let shared_nested = input["shared_nested_contract"].as_bool() == Some(true);
    let root_only_fixture = input["include_nested_contracts"].as_bool() == Some(false);
    if root_only_fixture {
        let child = package.join("child");
        fs::create_dir_all(&child).map_err(|error| error.to_string())?;
        write(
            child.join("dataset.json"),
            r#"{"version":1,"cases":[{"id":"child","input":{"value":2},"expected":{"value":2}}]}"#,
        )?;
        write(
            child.join("validator.py"),
            "def validate(value): return value\n",
        )?;
        write(
            child.join("open-contract.yaml"),
            "version: '1'\nname: child\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
        )?;
        manifest["nested_contracts"] = json!(["child/open-contract.yaml"]);
        manifest["filesystem"]["read"] =
            json!(["dataset.json", "validator.py", "child/open-contract.yaml"]);
        return Ok(());
    }
    if shared_nested {
        for (directory, name) in [("left", "left"), ("right", "right")] {
            let child = package.join(directory);
            fs::create_dir_all(&child).map_err(|error| error.to_string())?;
            write(
                child.join("dataset.json"),
                r#"{"version":1,"cases":[{"id":"branch","input":{"value":1},"expected":{"value":1},"coverage_fingerprint":"880ae786fa1c0630fe9158b7962fd8a2"}]}"#,
            )?;
            write(
                child.join("validator.py"),
                "def validate(value): return value\n",
            )?;
            write(
                child.join("open-contract.yaml"),
                &format!(
                    "version: '1'\nname: {name}\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nnested_contracts: [../shared/open-contract.yaml]\nfilesystem:\n  read: [dataset.json, validator.py, ../shared/open-contract.yaml]\n  write: []\n"
                ),
            )?;
        }
        let shared = package.join("shared");
        fs::create_dir_all(&shared).map_err(|error| error.to_string())?;
        write(
            shared.join("dataset.json"),
            r#"{"version":1,"cases":[{"id":"shared","input":{"value":1},"expected":{"value":1},"coverage_fingerprint":"880ae786fa1c0630fe9158b7962fd8a2"}]}"#,
        )?;
        write(
            shared.join("validator.py"),
            "def validate(value): return value\n",
        )?;
        write(
            shared.join("open-contract.yaml"),
            "version: '1'\nname: shared\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: []}\n",
        )?;
        manifest["nested_contracts"] =
            json!(["left/open-contract.yaml", "right/open-contract.yaml"]);
        manifest["filesystem"]["read"] =
            json!(["dataset.json", "validator.py", "left", "right", "shared"]);
        return Ok(());
    }
    if !duplicate_name && !cycle && !nested_contract {
        return Ok(());
    }
    let nested_manifest_path = input["nested_manifest_path"]
        .as_str()
        .unwrap_or("child/open-contract.yaml");
    let nested_manifest_path = Path::new(nested_manifest_path);
    let nested_manifest = package.join(nested_manifest_path);
    let child = nested_manifest
        .parent()
        .ok_or_else(|| "nested manifest path must have a parent".to_string())?;
    fs::create_dir_all(child).map_err(|error| format!("create nested fixture: {error}"))?;
    write(
        child.join("dataset.json"),
        r#"{"version":1,"cases":[{"id":"child","input":{},"expected":{},"coverage_fingerprint":"880ae786fa1c0630fe9158b7962fd8a2"}]}"#,
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
    let child_filename = nested_manifest_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "nested manifest path must have a filename".to_string())?;
    let nested = if cycle {
        format!("nested_contracts: [{child_filename}]\n")
    } else {
        String::new()
    };
    let child_manifest = format!(
        "version: '1'\nname: {name}\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\n{nested}filesystem:\n  read: [dataset.json, validator.py{}]\n  write: []\n",
        if cycle { format!(", {child_filename}") } else { String::new() }
    );
    write(nested_manifest.clone(), &child_manifest)?;
    if input["bad_nested_manifest"].as_bool() == Some(true) {
        write(nested_manifest, "version: [\n")?;
    }
    manifest["nested_contracts"] = json!([nested_manifest_path]);
    let reads = if input["support"].as_bool() == Some(true) || input["support_path"].is_string() {
        let support = input["support_path"]
            .as_str()
            .map_or("support.py", |value| value);
        json!([
            "dataset.json",
            "validator.py",
            support,
            nested_manifest_path
        ])
    } else {
        json!(["dataset.json", "validator.py", nested_manifest_path])
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
        "dataset-adapter-error-expected-as-output"
        | "dataset-adapter-error-output-mismatch"
        | "dataset-adapter-unexpected-error"
        | "dataset-adapter-step-error-output"
        | "dataset-adapter-step-predicate-mismatch"
        | "dataset-adapter-multiple-step-error-outputs"
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
        | "dataset-adapter-entity-coverage-source-undeclared"
        | "dataset-adapter-native-coverage-no-sources"
        | "dataset-adapter-native-coverage-non-string-source"
        | "dataset-adapter-native-coverage-undeclared-source"
        | "dataset-adapter-coverage-lines-undeclared-source"
        | "dataset-adapter-multiple-coverage-lines"
        | "dataset-adapter-coverage-source-unavailable"
        | "dataset-adapter-coverage-line-out-of-range"
        | "dataset-adapter-coverage-metric-nested-paths"
        | "dataset-adapter-covered-line-outside-executable"
        | "dataset-adapter-no-line-coverage"
        | "dataset-adapter-missing-step-coverage"
        | "dataset-adapter-missing-batch"
        | "dataset-adapter-error"
        | "dataset-adapter-root-only-contract-tree"
        | "dataset-adapter-shared-nested-contract-runs-once"
        | "dataset-adapter-wrong-expected-error" => Ok(()),
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
            write(
                child.join("validator.py"),
                "def validate(value): return value\n",
            )?;
            let child_manifest = input["child_manifest_yaml"]
                .as_str()
                .ok_or_else(|| "nested case requires child_manifest_yaml".to_string())?;
            write(child.join("open-contract.yaml"), child_manifest)?;
            Ok(())
        }
        id if id.starts_with("manifest-adapter-") || id.starts_with("supplemental-") => {
            let script = match id {
                "manifest-adapter-executes-batch" => {
                    r#"import json, sys
batch = json.load(sys.stdin)
requests = batch["requests"]
print(json.dumps({
    "results": [{"key": request["key"], "output": request["input"]} for request in requests],
    "coverage": {"format": "bootstrap-dataset", "sources": [batch["coverage_sources"][0]]},
    "coverage_scopes": [
        {"key": request["key"], "files": [{"path": batch["coverage_sources"][0], "lines": [1]}]}
        for request in requests
    ],
    "coverage_lines": [{"path": batch["coverage_sources"][0], "executable_lines": [1], "covered_lines": [1]}]
}))
"#
                }
                "manifest-adapter-command-fails" => "import sys\nsys.exit(1)\n",
                "manifest-adapter-command-fails-with-stdout" => {
                    "print('stdout diagnostic')\nraise SystemExit(1)\n"
                }
                "manifest-adapter-command-fails-with-stderr" => {
                    "import sys\nprint('stderr diagnostic', file=sys.stderr)\nsys.exit(1)\n"
                }
                "manifest-adapter-returns-invalid-json" => "print('{')\n",
                "manifest-adapter-returns-invalid-batch-data" => {
                    "import json\nprint(json.dumps({'results': 'not-an-array'}))\n"
                }
                "manifest-adapter-returns-invalid-coverage-data" => {
                    r#"import json, sys
batch=json.load(sys.stdin); request=batch["requests"][0]; source=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":request["key"],"output":request["input"]}],"coverage_lines":[{"path":source,"executable_lines":[1],"covered_lines":[2]}]}))
"#
                }
                "manifest-adapter-returns-no-results" => {
                    "import json\nprint(json.dumps({'results': []}))\n"
                }
                "manifest-adapter-missing-execution" => "print('{}')\n",
                "manifest-adapter-command-cannot-start" => "print('{}')\n",
                "supplemental-coverage-is-reported" => {
                    r#"import json, sys
batch = json.load(sys.stdin)
requests = batch["requests"]
print(json.dumps({
    "results": [{"key": request["key"], "output": request["input"]} for request in requests],
    "coverage": {"format": "bootstrap-dataset", "sources": [batch["coverage_sources"][0]]},
    "coverage_scopes": [
        {"key": request["key"], "files": [{"path": batch["coverage_sources"][0], "lines": [1]}]}
        for request in requests
    ],
    "coverage_lines": [{"path": batch["coverage_sources"][0], "executable_lines": [1], "covered_lines": [1]}],
    "additional_test_coverage": {"executed": True, "passed": True, "native_report": {"format": "fixture"}},
    "additional_coverage_lines": [{"path": batch["coverage_sources"][0], "executable_lines": [1, 2], "covered_lines": [2]}]
}))
"#
                }
                "supplemental-tests-low-dataset-coverage" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":True}}))
"#
                }
                "supplemental-coverage-without-tests" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":True}}))
"#
                }
                "supplemental-coverage-missing-report" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}]}))
"#
                }
                "supplemental-tests-failed" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":True,"passed":False,"error":"supplemental test failure"}}))
"#
                }
                "supplemental-tests-skipped" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}]}))
"#
                }
                "supplemental-tests-no-executable-lines" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[]}))
"#
                }
                "supplemental-tests-coverage-without-run" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[1]}],"additional_coverage_lines":[{"path":src,"executable_lines":[1,2],"covered_lines":[2]}]}))
"#
                }
                "supplemental-tests-not-executed" => {
                    r#"import json, sys
batch=json.load(sys.stdin); requests=batch["requests"]; src=batch["coverage_sources"][0]
print(json.dumps({"results":[{"key":r["key"],"output":r["input"]} for r in requests],"coverage":{"sources":[src]},"coverage_scopes":[{"key":r["key"],"files":[{"path":src,"lines":[1]}]} for r in requests],"coverage_lines":[{"path":src,"executable_lines":[1],"covered_lines":[1]}],"additional_test_coverage":{"executed":False,"passed":True}}))
"#
                }
                _ => {
                    return Err(format!(
                        "no adapter fixture is defined for dataset case {id:?}"
                    ))
                }
            };
            let adapter = "adapter.runner";
            write(package.join(adapter), script)?;
            manifest["coverage_sources"]
                .as_array_mut()
                .unwrap()
                .push(json!(adapter));
            manifest["filesystem"]["read"] = json!(["dataset.json", "validator.py", adapter]);
            if input["supplemental_tests"].as_bool() == Some(true) {
                manifest["filesystem"]["read"]
                    .as_array_mut()
                    .ok_or_else(|| "fixture read list must be an array".to_string())?
                    .push(json!("tests/supplemental.py"));
            }
            if id.starts_with("supplemental-tests-") || id == "supplemental-coverage-missing-report"
            {
                fs::create_dir_all(package.join("tests"))
                    .map_err(|error| format!("create supplemental test directory: {error}"))?;
                write(
                    package.join("tests/supplemental.py"),
                    "def run(): return True\n",
                )?;
                manifest["supplemental_tests"] = json!(["tests/supplemental.py"]);
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
    predicate: Option<&str>,
) -> Result<(), String> {
    let actual = match result {
        Ok(report) => serde_json::to_value(report)
            .map_err(|error| format!("serialize batch report: {error}"))?,
        Err(error) => json!({"error": error}),
    };
    check_public_output(&actual, expected, predicate)
}

fn check_public_output(
    actual: &Value,
    expected: &Value,
    predicate: Option<&str>,
) -> Result<(), String> {
    match match_output(actual, Some(expected), predicate) {
        Ok(MatchKind::Expected | MatchKind::ExpectedAndPredicate | MatchKind::Predicate) => Ok(()),
        Err(error) => Err(format!(
            "public output did not match: {error}; actual: {actual}"
        )),
    }
}

/// Deterministic adapter used only to drive the bootstrap contract's public
/// dataset inputs through the batch API.
struct IdentityDatasetAdapter;

struct ScenarioDatasetAdapter {
    id: String,
    coverage_metrics: std::collections::BTreeMap<String, runner_batch::CoverageMetricCounts>,
    coverage_entities: Vec<Value>,
}

impl BatchAdapter for ScenarioDatasetAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        let mut outputs = IdentityDatasetAdapter.execute_tree(tree, backend, batches)?;
        if self.id == "dataset-adapter-missing-result-dataset-identity" {
            for batch in &mut outputs {
                for result in &mut batch.results {
                    result.key.dataset_id = None;
                }
            }
        }
        if self.id == "dataset-adapter-invalid-result-dataset-identity" {
            for batch in &mut outputs {
                for result in &mut batch.results {
                    result.key.dataset_id = Some("9999".into());
                }
            }
        }
        if self.id == "dataset-adapter-missing-scope-dataset-identity" {
            for batch in &mut outputs {
                for scope in &mut batch.coverage_scopes {
                    scope.key.dataset_id = None;
                }
            }
        }
        if self.id == "dataset-adapter-invalid-scope-dataset-identity" {
            for batch in &mut outputs {
                for scope in &mut batch.coverage_scopes {
                    scope.key.dataset_id = Some("9999".into());
                }
            }
        }
        for (contract_batch, batch) in batches.iter().zip(&mut outputs) {
            batch.coverage_metrics = self.coverage_metrics.clone();
            batch.coverage_entities = self.coverage_entities.clone();
            if self.id == "dataset-adapter-union-coverage" {
                if batches.len() != 1 || contract_batch.requests.len() != 2 {
                    return Err("datasets were not executed together for contract coverage".into());
                }
                batch.coverage_lines = vec![runner_batch::CoverageLineFile {
                    path: contract_batch.manifest.manifest.coverage_sources[0].clone(),
                    executable_lines: vec![1, 2],
                    covered_lines: vec![1, 2],
                }];
            }
            if self.id == "dataset-adapter-coverage-metric-below" {
                batch.coverage_lines = vec![runner_batch::CoverageLineFile {
                    path: contract_batch.manifest.manifest.coverage_sources[0].clone(),
                    executable_lines: vec![1, 2],
                    covered_lines: vec![1],
                }];
            }
            if self.id == "dataset-adapter-coverage-source-unavailable" {
                fs::remove_file(contract_batch.manifest.package_root.join("validator.py"))
                    .map_err(|error| format!("remove coverage source fixture: {error}"))?;
            }
            for result in &mut batch.results {
                match self.id.as_str() {
                    "dataset-adapter-error-expected-as-output"
                    | "dataset-adapter-error-output-mismatch"
                    | "dataset-adapter-unexpected-error"
                    | "dataset-adapter-step-error-output"
                    | "dataset-adapter-step-predicate-mismatch"
                    | "dataset-adapter-multiple-step-error-outputs" => {
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
                "dataset-adapter-preserves-input-and-context" => {
                    for result in &mut batch.results {
                        let request = contract_batch
                            .requests
                            .iter()
                            .find(|request| request.key == result.key)
                            .ok_or_else(|| {
                                "scenario adapter received an unknown result key".to_string()
                            })?;
                        result.output = Some(json!({
                            "input": request.input,
                            "setup": request.setup,
                            "parallel": request.parallel,
                        }));
                    }
                }
                "dataset-adapter-omits-result" => batch.results.clear(),
                "dataset-adapter-missing-coverage" => batch.coverage_scopes.clear(),
                "dataset-adapter-duplicate-result" => {
                    if let Some(result) = batch.results.first().cloned() {
                        batch.results.push(result);
                    }
                }
                "dataset-adapter-extra-result" => batch.results.push(AdapterResult {
                    key: ExecutionKey {
                        case_id: "unrequested".into(),
                        step_id: None,
                        dataset_id: contract_batch
                            .requests
                            .first()
                            .and_then(|request| request.key.dataset_id.clone()),
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
                "dataset-adapter-native-coverage-undeclared-source" => {
                    batch.coverage = Some(json!({"sources": ["validator.py", "private.py"]}));
                }
                "dataset-adapter-coverage-lines-undeclared-source" => {
                    if let Some(file) = batch.coverage_lines.first_mut() {
                        file.path = "private.py".into();
                    }
                }
                "dataset-adapter-multiple-coverage-lines" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope
                            .files
                            .iter_mut()
                            .find(|file| file.path == "support.py")
                        {
                            file.lines = vec![1, 2];
                        }
                    }
                }
                "dataset-adapter-coverage-line-out-of-range" => {
                    if let Some(scope) = batch.coverage_scopes.first_mut() {
                        if let Some(file) = scope.files.first_mut() {
                            file.lines = vec![u32::MAX];
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
                        scope.key.case_id = "unrequested".into();
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
                "dataset-adapter-covered-line-outside-executable" => {
                    if let Some(file) = batch.coverage_lines.first_mut() {
                        file.executable_lines = vec![1];
                        file.covered_lines = vec![2];
                    }
                }
                "dataset-adapter-missing-step-coverage" => {
                    batch
                        .coverage_scopes
                        .retain(|scope| scope.key.step_id.is_none());
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
                    "sources": std::iter::once(&batch.manifest.manifest.coverage_sources[0])
                        .chain(batch.manifest.manifest.coverage_sources.iter().skip(1)).collect::<Vec<_>>(),
                })),
                coverage_scopes: batch
                    .requests
                    .iter()
                    .map(|request| CoverageScope {
                        key: request.key.clone(),
                        files: std::iter::once(&batch.manifest.manifest.coverage_sources[0])
                            .chain(batch.manifest.manifest.coverage_sources.iter().skip(1))
                            .map(|path| CoverageFile {
                                path: path.clone(),
                                lines: vec![1],
                            })
                            .collect(),
                    })
                    .collect(),
                coverage_lines: std::iter::once(&batch.manifest.manifest.coverage_sources[0])
                    .chain(batch.manifest.manifest.coverage_sources.iter().skip(1))
                    .map(|path| runner_batch::CoverageLineFile {
                        path: path.clone(),
                        executable_lines: vec![1],
                        covered_lines: vec![1],
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
