use runner_isolation::{Backend, StagedTree};
use runner_manifest::{is_allowed, validate_package_path, Manifest, ManifestPlan};
use runner_verification::{match_output, MatchKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[jsonschema::validator(path = "../../../schemas/dataset.v1.schema.json")]
struct DatasetSchema;

#[jsonschema::validator(path = "../../../schemas/adapter-coverage.v1.schema.json")]
struct AdapterCoverageSchema;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub version: u32,
    pub cases: Vec<CaseSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CaseSpec {
    Call(CallSpec),
    Sequence(SequenceSpec),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallSpec {
    pub id: String,
    pub input: Value,
    pub expected: Option<Value>,
    pub predicate: Option<String>,
    pub coverage_fingerprint: Option<String>,
    pub setup: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceSpec {
    pub id: String,
    pub steps: Vec<StepSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub id: String,
    pub input: Value,
    pub expected: Option<Value>,
    pub predicate: Option<String>,
    pub coverage_fingerprint: Option<String>,
    pub setup: Option<String>,
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct ExecutionKey {
    pub case_id: String,
    pub step_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub key: ExecutionKey,
    pub input: Value,
    pub setup: Option<String>,
    pub parallel: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterResult {
    pub key: ExecutionKey,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AdapterBatch {
    pub results: Vec<AdapterResult>,
    #[serde(default)]
    pub coverage: Option<Value>,
    #[serde(default)]
    pub coverage_scopes: Vec<CoverageScope>,
    /// Language-neutral line accounting for the declared contract sources.
    /// Native reports remain adapter-owned; this normalized form is for the
    /// Rust runner's uncovered-line report.
    #[serde(default)]
    pub coverage_lines: Vec<CoverageLineFile>,
    /// Adapter-owned summary for explicitly declared supplemental tests.
    #[serde(default)]
    pub additional_test_coverage: Option<Value>,
    /// Normalized source-line data from supplemental tests only.
    #[serde(default)]
    pub additional_coverage_lines: Vec<CoverageLineFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageLineFile {
    pub path: String,
    pub executable_lines: Vec<u32>,
    pub covered_lines: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageFile {
    pub path: String,
    pub lines: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageScope {
    pub key: ExecutionKey,
    pub files: Vec<CoverageFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageFingerprint {
    pub files: Vec<CoverageFile>,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_matches: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct ContractBatch {
    pub manifest: ManifestPlan,
    pub package_prefix: PathBuf,
    pub requests: Vec<ExecutionRequest>,
    pub coverage_fingerprints: bool,
}

/// Resolve the filesystem and command plan for a contract tree without
/// executing its declared commands.
pub fn show_isolation_plan(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
) -> Result<String, String> {
    let root = ManifestPlan::load(manifest_path)?;
    let tree = StagedTree::create_for_backend(&root, backend, isolation_runtime)?;
    let mut output = String::new();

    let backend_name = match backend {
        Backend::TempDir => "tempdir",
        Backend::None => "none",
        Backend::Bubblewrap => "bubblewrap",
        Backend::Landlock => "landlock",
    };
    output.push_str(&format!("Isolation: {backend_name}\n"));
    if backend == Backend::None {
        output.push_str("Filesystem enforcement: none (host access is unrestricted)\n");
    } else if backend == Backend::TempDir {
        output.push_str(
            "Filesystem enforcement: none (contract files are copied to a temporary tree)\n",
        );
    }
    output.push_str("Contract read-only paths:\n");
    for path in &tree.read {
        let full_path = tree.root.join(path);
        let kind = if full_path.is_dir() {
            "recursive"
        } else {
            "file"
        };
        output.push_str(&format!("  {path} ({kind})\n"));
    }
    output.push_str("Contract writable paths:\n");
    for path in &tree.write {
        let full_path = tree.root.join(path);
        let kind = if full_path.is_dir() {
            "recursive"
        } else {
            "file"
        };
        output.push_str(&format!("  {path} ({kind})\n"));
    }

    for (manifest, prefix) in &tree.manifests {
        let execution = manifest.manifest.execution.as_ref().ok_or_else(|| {
            format!(
                "contract {} is missing execution.command",
                manifest.manifest.name
            )
        })?;
        let working_directory = tree.root.join(prefix);
        let command = tree.plan_command_at(
            backend,
            &working_directory,
            &execution.command,
            manifest.manifest.language_package.as_deref(),
        )?;
        output.push_str(&format!("Contract {}:\n", manifest.manifest.name));
        output.push_str(&format!(
            "  Working directory: {}\n",
            command.working_directory.display()
        ));
        output.push_str(&format!("  Command argv: {:?}\n", command.command));
        if let Some(executable) = command.resolved_executable {
            output.push_str(&format!(
                "  Resolved executable: {}\n",
                executable.display()
            ));
        }
        output.push_str("  Runtime read-only paths:\n");
        for path in command.runtime_roots {
            let kind = if path.is_dir() { "recursive" } else { "file" };
            output.push_str(&format!("    {} ({kind})\n", path.display()));
        }
        if backend == Backend::Bubblewrap {
            output.push_str("  Bubblewrap fixed mounts: /proc, /dev, /tmp, /contract\n");
            output.push_str("  Bubblewrap contract root: /contract/<randomized-root>\n");
        }
    }
    Ok(output)
}

/// Contract command contract: each command receives one complete flattened
/// dataset batch and returns raw outputs/errors and coverage. Rust owns matching
/// and predicate verification after the batch.
pub trait BatchAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String>;
}

/// Executes each contract's declared command once inside the established
/// isolation boundary. Nested contracts share the same boundary.
pub struct ManifestBatchAdapter;

impl BatchAdapter for ManifestBatchAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        batches
            .iter()
            .map(|batch| {
                let manifest = &batch.manifest.manifest;
                let execution = manifest.execution.as_ref().ok_or_else(|| {
                    format!("contract {} is missing execution.command", manifest.name)
                })?;
                let package_root = tree.root.join(&batch.package_prefix);
                let input = if batch.coverage_fingerprints {
                    batch_input(manifest, &batch.requests)
                } else {
                    batch_input_with_fingerprints(manifest, &batch.requests, false)
                }
                .to_string()
                .into_bytes();
                let output = tree.run_command_at(
                    backend,
                    &package_root,
                    &execution.command,
                    &input,
                    manifest.language_package.as_deref(),
                )?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    let diagnostic = if stderr.is_empty() {
                        if stdout.is_empty() {
                            "command produced no stdout or stderr"
                        } else {
                            &stdout
                        }
                    } else {
                        &stderr
                    };
                    return Err(format!(
                        "contract {} execution.command failed ({}): {diagnostic}",
                        manifest.name, output.status
                    ));
                }
                decode_batch_output(&output.stdout, &manifest.name)
            })
            .collect()
    }
}

fn decode_batch_output(bytes: &[u8], contract: &str) -> Result<AdapterBatch, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("language adapter returned invalid batch JSON: {e}"))?;
    let response: AdapterBatch = serde_json::from_value(value.clone())
        .map_err(|e| format!("language adapter returned invalid batch data: {e}"))?;
    if response.results.is_empty() {
        return Err(format!("contract {contract} returned no batch results"));
    }
    validate_adapter_coverage_payload(&value, contract)?;
    Ok(response)
}

/// Validate normalized coverage before it reaches threshold calculations.
pub fn validate_adapter_coverage_payload(value: &Value, contract: &str) -> Result<(), String> {
    let coverage = json!({
        "coverage": value.get("coverage").cloned().unwrap_or(Value::Null),
        "coverage_lines": value.get("coverage_lines").cloned().unwrap_or(Value::Null),
        "additional_test_coverage": value.get("additional_test_coverage").cloned().unwrap_or(Value::Null),
        "additional_coverage_lines": value.get("additional_coverage_lines").cloned().unwrap_or_else(|| json!([])),
    });
    if let Some(error) = AdapterCoverageSchema::iter_errors(&coverage).next() {
        return Err(format!(
            "contract {contract} returned invalid adapter coverage at {}: {}",
            error.instance_path(),
            error
        ));
    }
    for field in ["coverage_lines", "additional_coverage_lines"] {
        let files = coverage[field].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let mut paths = std::collections::HashSet::new();
        for (index, file) in files.iter().enumerate() {
            let path = file["path"].as_str().unwrap_or_default();
            if !paths.insert(path) {
                return Err(format!("contract {contract} returned invalid adapter coverage at /{field}/{index}/path: duplicate file path {path:?}"));
            }
            let executable = file["executable_lines"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let covered = file["covered_lines"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if !strictly_increasing(executable) {
                return Err(format!("contract {contract} returned invalid adapter coverage at /{field}/{index}/executable_lines: line numbers must be sorted and unique"));
            }
            if !strictly_increasing(covered) {
                return Err(format!("contract {contract} returned invalid adapter coverage at /{field}/{index}/covered_lines: line numbers must be sorted and unique"));
            }
            if covered.iter().any(|line| !executable.contains(line)) {
                return Err(format!("contract {contract} returned invalid adapter coverage at /{field}/{index}/covered_lines: every covered line must be executable"));
            }
            let source_path = Path::new(path);
            if source_path.is_absolute()
                || source_path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(format!("contract {contract} returned invalid adapter coverage at /{field}/{index}/path: path must be package-relative"));
            }
        }
    }
    Ok(())
}

fn adapter_coverage_value(batch: &AdapterBatch) -> Value {
    json!({
        "coverage": batch.coverage,
        "coverage_lines": batch.coverage_lines,
        "additional_test_coverage": batch.additional_test_coverage,
        "additional_coverage_lines": batch.additional_coverage_lines,
    })
}

fn strictly_increasing(lines: &[Value]) -> bool {
    lines
        .windows(2)
        .all(|pair| pair[0].as_u64() < pair[1].as_u64())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    pub output: Option<Value>,
    pub passed: bool,
    pub matched_by: Option<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractResult {
    pub contract: String,
    pub case_id: String,
    pub output: Option<Value>,
    pub passed: bool,
    pub matched_by: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageRecord {
    pub contract: String,
    #[serde(skip)]
    pub dataset_path: PathBuf,
    pub minimum_line_percent: f64,
    pub dataset_covered_lines: usize,
    pub dataset_executable_lines: usize,
    pub combined_covered_lines: usize,
    pub combined_executable_lines: usize,
    pub line_threshold_met: bool,
    pub native_report: Value,
    pub declared_sources: Vec<String>,
    pub covered_sources: Vec<String>,
    pub missing_sources: Vec<String>,
    #[serde(default)]
    pub line_coverage: Vec<CoverageLineFile>,
    #[serde(default)]
    pub additional_test_coverage: Value,
    #[serde(default)]
    pub additional_line_coverage: Vec<CoverageLineFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BatchReport {
    pub results: Vec<ContractResult>,
    /// Raw native reports from the root and every child run in this shared
    /// isolation. Adapters own merging reports whose formats are language-specific.
    pub coverage: Vec<CoverageRecord>,
}

pub fn run_contract_tree(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    run_contract_tree_with_runtime(manifest_path, backend, None, adapter)
}

/// Run a contract tree with an optional override for the selected isolation
/// backend's system runtime manifest.
pub fn run_contract_tree_with_runtime(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    run_contract_tree_with_options(manifest_path, backend, isolation_runtime, true, adapter)
}

/// Run a contract tree with optional per-entry coverage fingerprint checks.
/// Aggregate dataset line coverage is always collected.
pub fn run_contract_tree_with_options(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
    coverage_fingerprints: bool,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    run_contract_tree_with_fingerprint_mode(
        manifest_path,
        backend,
        isolation_runtime,
        if coverage_fingerprints {
            FingerprintMode::Check
        } else {
            FingerprintMode::Disabled
        },
        adapter,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FingerprintMode {
    Disabled,
    Check,
    Update,
}

/// Run a contract tree while checking fingerprints, updating them, or
/// disabling per-entry collection. Aggregate dataset line coverage is always
/// collected.
pub fn run_contract_tree_with_fingerprint_mode(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
    fingerprint_mode: FingerprintMode,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    let coverage_fingerprints = fingerprint_mode != FingerprintMode::Disabled;
    let require_fingerprints = fingerprint_mode == FingerprintMode::Check;
    let compare_fingerprints = fingerprint_mode == FingerprintMode::Check;
    let root = ManifestPlan::load(manifest_path)?;
    let tree = StagedTree::create_for_backend(&root, backend, isolation_runtime)?;
    let mut batches = Vec::new();
    let mut checks_by_contract = Vec::new();
    let mut names = std::collections::HashSet::new();
    for (manifest, package_prefix) in &tree.manifests {
        if !names.insert(manifest.manifest.name.clone()) {
            return Err(format!(
                "duplicate contract name in nested tree: {}",
                manifest.manifest.name
            ));
        }
        let data_path = manifest.package_root.join(&manifest.manifest.files.dataset);
        let dataset = load_dataset(&data_path)?;
        let (requests, checks) = flatten_dataset_with_fingerprints(&dataset, require_fingerprints)?;
        batches.push(ContractBatch {
            manifest: manifest.clone(),
            package_prefix: package_prefix.to_path_buf(),
            requests,
            coverage_fingerprints,
        });
        checks_by_contract.push(checks);
    }
    let adapter_batches = adapter.execute_tree(&tree, backend, &batches)?;
    if adapter_batches.len() != batches.len() {
        return Err(format!(
            "adapter returned {} contract batches for {} requested packages",
            adapter_batches.len(),
            batches.len()
        ));
    }
    let mut report = BatchReport::default();
    for ((batch, checks), mut adapter_batch) in
        batches.iter().zip(checks_by_contract).zip(adapter_batches)
    {
        let adapter_value = adapter_coverage_value(&adapter_batch);
        validate_adapter_coverage_payload(&adapter_value, &batch.manifest.manifest.name)?;
        let additional_test_coverage = validate_additional_test_coverage(batch, &adapter_batch)?;
        prefix_line_coverage_paths(&batch.package_prefix, &mut adapter_batch.coverage_lines);
        prefix_line_coverage_paths(
            &batch.package_prefix,
            &mut adapter_batch.additional_coverage_lines,
        );
        prefix_native_coverage_sources(&batch.package_prefix, &mut adapter_batch.coverage);
        let (dataset_covered_lines, dataset_executable_lines) =
            line_coverage_totals(&adapter_batch.coverage_lines);
        let combined_lines = merge_line_coverage(
            &adapter_batch.coverage_lines,
            &adapter_batch.additional_coverage_lines,
        );
        let (combined_covered_lines, combined_executable_lines) =
            line_coverage_totals(&combined_lines);
        let minimum_line_percent = batch.manifest.manifest.coverage.minimum_line_percent;
        let combined_percent = coverage_percent(combined_covered_lines, combined_executable_lines);
        let line_threshold_met = combined_executable_lines > 0
            && combined_percent + f64::EPSILON >= minimum_line_percent;
        let outputs = index_results(adapter_batch.results)?;
        let mut fingerprint_source_lines = std::collections::HashMap::new();
        let coverage_scopes = if coverage_fingerprints {
            index_coverage_scopes(
                batch,
                adapter_batch.coverage_scopes,
                &mut fingerprint_source_lines,
            )?
        } else {
            std::collections::HashMap::new()
        };
        let declared_sources = declared_sources(batch);
        let covered_sources = adapter_batch
            .coverage
            .as_ref()
            .and_then(|coverage| coverage.get("sources"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();
        let missing_sources = declared_sources
            .iter()
            .filter(|source| !covered_sources.contains(*source))
            .cloned()
            .collect();
        report.coverage.push(CoverageRecord {
            contract: batch.manifest.manifest.name.clone(),
            dataset_path: batch
                .manifest
                .package_root
                .join(&batch.manifest.manifest.files.dataset),
            minimum_line_percent,
            dataset_covered_lines,
            dataset_executable_lines,
            combined_covered_lines,
            combined_executable_lines,
            line_threshold_met,
            native_report: adapter_batch.coverage.unwrap_or(Value::Null),
            declared_sources,
            covered_sources: {
                let mut sources = covered_sources.into_iter().collect::<Vec<_>>();
                sources.sort();
                sources
            },
            missing_sources,
            line_coverage: adapter_batch.coverage_lines,
            additional_test_coverage,
            additional_line_coverage: adapter_batch.additional_coverage_lines,
        });
        report.results.extend(verify_dataset(
            &batch.manifest.manifest,
            checks,
            outputs,
            &coverage_scopes,
            &fingerprint_source_lines,
            coverage_fingerprints,
            compare_fingerprints,
        )?);
    }
    Ok(report)
}

fn prefix_line_coverage_paths(prefix: &Path, files: &mut [CoverageLineFile]) {
    if prefix.as_os_str().is_empty() || prefix == Path::new(".") {
        return;
    }
    for file in files {
        file.path = prefix.join(&file.path).to_string_lossy().replace('\\', "/");
    }
}

fn prefix_native_coverage_sources(prefix: &Path, coverage: &mut Option<Value>) {
    if prefix.as_os_str().is_empty() || prefix == Path::new(".") {
        return;
    }
    let Some(sources) = coverage
        .as_mut()
        .and_then(|report| report.get_mut("sources"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for source in sources {
        if let Some(path) = source.as_str().map(str::to_owned) {
            *source = Value::String(prefix.join(path).to_string_lossy().replace('\\', "/"));
        }
    }
}

fn declared_sources(batch: &ContractBatch) -> Vec<String> {
    let mut sources = vec![batch.manifest.manifest.files.validator.clone()];
    sources.extend(batch.manifest.manifest.files.support.iter().cloned());
    sources
        .into_iter()
        .filter(|source| is_source_file(source))
        .map(|source| {
            batch
                .package_prefix
                .join(source)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn is_source_file(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("py" | "js" | "mjs" | "cjs" | "rs")
    )
}

fn validate_additional_test_coverage(
    batch: &ContractBatch,
    adapter_batch: &AdapterBatch,
) -> Result<Value, String> {
    let test_files = &batch.manifest.manifest.files.supplemental_tests;
    if test_files.is_empty() {
        if adapter_batch.additional_test_coverage.is_some()
            || !adapter_batch.additional_coverage_lines.is_empty()
        {
            return Err(format!(
                "adapter returned supplemental coverage for contract {} without manifest-declared tests",
                batch.manifest.manifest.name
            ));
        }
        return Ok(Value::Null);
    }

    let (covered, executable) = line_coverage_totals(&adapter_batch.coverage_lines);
    let dataset_percent = if executable == 0 {
        0.0
    } else {
        covered as f64 * 100.0 / executable as f64
    };
    let adapter_report = adapter_batch.additional_test_coverage.as_ref();
    if dataset_percent < 90.0 {
        if adapter_report.and_then(|report| report["executed"].as_bool()) == Some(true) {
            return Err(format!(
                "adapter ran supplemental tests for {} with only {:.1}% dataset line coverage; 90% is required",
                batch.manifest.manifest.name, dataset_percent
            ));
        }
        if !adapter_batch.additional_coverage_lines.is_empty() {
            return Err(format!(
                "adapter returned supplemental coverage for {} without running its tests",
                batch.manifest.manifest.name
            ));
        }
        return Ok(json!({
            "status": "skipped",
            "executed": false,
            "passed": true,
            "reason": format!("dataset-only line coverage {:.1}% is below the 90% threshold", dataset_percent),
            "dataset_line_coverage_percent": dataset_percent,
            "test_files": test_files,
            "files": coverage_line_json(&adapter_batch.additional_coverage_lines),
        }));
    }

    let adapter_report = adapter_report.ok_or_else(|| {
        format!(
            "adapter omitted supplemental test results for contract {} despite {:.1}% dataset line coverage",
            batch.manifest.manifest.name, dataset_percent
        )
    })?;
    if adapter_report["executed"].as_bool() != Some(true) {
        return Err(format!(
            "adapter did not execute manifest-declared supplemental tests for contract {}",
            batch.manifest.manifest.name
        ));
    }
    let passed = adapter_report["passed"].as_bool().unwrap_or(false);
    Ok(json!({
        "status": if passed { "passed" } else { "failed" },
        "executed": true,
        "passed": passed,
        "error": adapter_report.get("error"),
        "native_report": adapter_report.get("native_report"),
        "dataset_line_coverage_percent": dataset_percent,
        "test_files": test_files,
        "files": coverage_line_json(&adapter_batch.additional_coverage_lines),
    }))
}

fn line_coverage_totals(files: &[CoverageLineFile]) -> (usize, usize) {
    let mut by_path = std::collections::BTreeMap::<
        String,
        (
            std::collections::BTreeSet<u32>,
            std::collections::BTreeSet<u32>,
        ),
    >::new();
    for file in files {
        let (executable, covered) = by_path.entry(file.path.clone()).or_default();
        executable.extend(file.executable_lines.iter().copied());
        covered.extend(file.covered_lines.iter().copied());
    }
    by_path.into_values().fold(
        (0, 0),
        |(covered_total, executable_total), (executable, covered)| {
            (
                covered_total + covered.intersection(&executable).count(),
                executable_total + executable.len(),
            )
        },
    )
}

fn merge_line_coverage(
    dataset: &[CoverageLineFile],
    supplemental: &[CoverageLineFile],
) -> Vec<CoverageLineFile> {
    let mut by_path = std::collections::BTreeMap::<
        String,
        (
            std::collections::BTreeSet<u32>,
            std::collections::BTreeSet<u32>,
        ),
    >::new();
    for file in dataset.iter().chain(supplemental) {
        let (executable, covered) = by_path.entry(file.path.clone()).or_default();
        executable.extend(file.executable_lines.iter().copied());
        covered.extend(file.covered_lines.iter().copied());
    }
    by_path
        .into_iter()
        .map(|(path, (executable, covered))| CoverageLineFile {
            path,
            executable_lines: executable.into_iter().collect(),
            covered_lines: covered.into_iter().collect(),
        })
        .collect()
}

fn coverage_percent(covered: usize, executable: usize) -> f64 {
    if executable == 0 {
        0.0
    } else {
        covered as f64 * 100.0 / executable as f64
    }
}

fn coverage_line_json(files: &[CoverageLineFile]) -> Vec<Value> {
    files
        .iter()
        .map(|file| {
            let executable = file
                .executable_lines
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            let covered = file
                .covered_lines
                .iter()
                .copied()
                .filter(|line| executable.contains(line))
                .collect::<std::collections::BTreeSet<_>>();
            let uncovered = executable.difference(&covered).copied().collect::<Vec<_>>();
            json!({
                "path": file.path,
                "executable_lines": executable.into_iter().collect::<Vec<_>>(),
                "covered_lines": covered.into_iter().collect::<Vec<_>>(),
                "uncovered_lines": uncovered,
            })
        })
        .collect()
}

fn index_coverage_scopes(
    batch: &ContractBatch,
    scopes: Vec<CoverageScope>,
    fingerprint_source_lines: &mut std::collections::HashMap<
        String,
        std::collections::BTreeMap<u32, String>,
    >,
) -> Result<std::collections::HashMap<ExecutionKey, CoverageFingerprint>, String> {
    let expected_keys = batch
        .requests
        .iter()
        .map(|request| request.key.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut allowed_sources = vec![batch.manifest.manifest.files.validator.clone()];
    allowed_sources.extend(batch.manifest.manifest.files.support.iter().cloned());
    let mut indexed = std::collections::HashMap::new();
    for scope in scopes {
        if !expected_keys.contains(&scope.key) {
            return Err(format!(
                "adapter returned coverage for unrequested execution key: {:?}",
                scope.key
            ));
        }
        for file in &scope.files {
            validate_package_path(&file.path)?;
            if !is_allowed(&file.path, &allowed_sources) {
                return Err(format!(
                    "adapter returned coverage for undeclared contract source: {}",
                    file.path
                ));
            }
        }
        let files = canonical_coverage_files(scope.files)?;
        let (fingerprint, source_lines) = coverage_hash(&files, &batch.manifest.package_root)?;
        for (path, lines) in source_lines {
            fingerprint_source_lines
                .entry(path)
                .or_default()
                .extend(lines);
        }
        let record = CoverageFingerprint {
            files,
            fingerprint,
            expected_fingerprint: None,
            baseline_matches: None,
        };
        if indexed.insert(scope.key.clone(), record).is_some() {
            return Err(format!(
                "adapter returned duplicate coverage scope: {:?}",
                scope.key
            ));
        }
    }
    Ok(indexed)
}

fn canonical_coverage_files(files: Vec<CoverageFile>) -> Result<Vec<CoverageFile>, String> {
    for file in &files {
        if file.lines.contains(&0) {
            return Err(format!(
                "coverage line numbers must be positive: {}",
                file.path
            ));
        }
    }
    Ok(merge_coverage_files(files))
}

fn merge_coverage_files(files: Vec<CoverageFile>) -> Vec<CoverageFile> {
    let mut by_path = std::collections::BTreeMap::<String, std::collections::BTreeSet<u32>>::new();
    for file in files {
        let lines = by_path.entry(file.path).or_default();
        lines.extend(file.lines);
    }
    by_path
        .into_iter()
        .filter_map(|(path, lines)| {
            (!lines.is_empty()).then(|| CoverageFile {
                path,
                lines: lines.into_iter().collect(),
            })
        })
        .collect()
}

fn coverage_hash(
    files: &[CoverageFile],
    package_root: &Path,
) -> Result<
    (
        String,
        std::collections::HashMap<String, std::collections::BTreeMap<u32, String>>,
    ),
    String,
> {
    let mut sources = Vec::with_capacity(files.len());
    let mut source_lines = std::collections::HashMap::new();
    for file in files {
        let source_path = package_root.join(&file.path);
        let source = fs::read_to_string(&source_path).map_err(|error| {
            format!(
                "cannot read source for coverage fingerprint {}: {error}",
                source_path.display()
            )
        })?;
        let source_file_lines = source.lines().collect::<Vec<_>>();
        let mut covered_lines = Vec::with_capacity(file.lines.len());
        let mut lines_by_number = std::collections::BTreeMap::new();
        for line_number in &file.lines {
            let Some(line) = line_number
                .checked_sub(1)
                .and_then(|index| source_file_lines.get(index as usize))
            else {
                return Err(format!(
                    "coverage line {line_number} is outside source file {}",
                    file.path
                ));
            };
            covered_lines.push((*line).to_owned());
            lines_by_number.insert(*line_number, (*line).to_owned());
        }
        source_lines.insert(file.path.clone(), lines_by_number);
        sources.push((file.path.clone(), covered_lines));
    }
    let encoded = coverage_source_lines_json(&sources);
    let digest = Sha256::digest(encoded);
    Ok((format!("{:x}", digest)[..32].to_owned(), source_lines))
}

fn coverage_hash_from_source_lines(
    files: &[CoverageFile],
    source_lines: &std::collections::HashMap<String, std::collections::BTreeMap<u32, String>>,
) -> String {
    let sources = files
        .iter()
        .map(|file| {
            let lines = file
                .lines
                .iter()
                .map(|number| source_lines[&file.path][number].clone())
                .collect();
            (file.path.clone(), lines)
        })
        .collect::<Vec<_>>();
    let encoded = coverage_source_lines_json(&sources);
    let digest = Sha256::digest(encoded);
    format!("{:x}", digest)[..32].to_owned()
}

fn coverage_source_lines_json(sources: &[(String, Vec<String>)]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.push(b'[');
    for (file_index, (path, lines)) in sources.iter().enumerate() {
        if file_index > 0 {
            encoded.push(b',');
        }
        encoded.extend_from_slice(b"{\"path\":");
        push_json_string(&mut encoded, path);
        encoded.extend_from_slice(b",\"lines\":[");
        for (line_index, line) in lines.iter().enumerate() {
            if line_index > 0 {
                encoded.push(b',');
            }
            push_json_string(&mut encoded, line);
        }
        encoded.extend_from_slice(b"]}");
    }
    encoded.push(b']');
    encoded
}

fn push_json_string(output: &mut Vec<u8>, value: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(b"\\\""),
            '\u{0008}' => output.extend_from_slice(b"\\b"),
            '\u{0009}' => output.extend_from_slice(b"\\t"),
            '\u{000a}' => output.extend_from_slice(b"\\n"),
            '\u{000c}' => output.extend_from_slice(b"\\f"),
            '\u{000d}' => output.extend_from_slice(b"\\r"),
            character if character <= '\u{001f}' => {
                let value = character as u8;
                output.extend_from_slice(b"\\u00");
                output.push(HEX[(value >> 4) as usize]);
                output.push(HEX[(value & 0x0f) as usize]);
            }
            character => {
                let mut buffer = [0; 4];
                output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    output.push(b'"');
}

fn coverage_for_key(
    scopes: &std::collections::HashMap<ExecutionKey, CoverageFingerprint>,
    key: &ExecutionKey,
    expected: Option<&str>,
    compare_expected: bool,
) -> (Option<CoverageFingerprint>, Option<String>) {
    let Some(mut coverage) = scopes.get(key).cloned() else {
        return (
            None,
            expected.map(|_| format!("coverage fingerprint unavailable for {:?}", key)),
        );
    };
    if expected.is_some() && coverage.files.iter().all(|file| file.lines.is_empty()) {
        return (
            Some(coverage),
            Some(format!(
                "coverage fingerprint has zero covered lines for {:?}",
                key
            )),
        );
    }
    if compare_expected {
        compare_fingerprint(&mut coverage, expected);
    } else {
        coverage.expected_fingerprint = expected.map(str::to_owned);
    }
    let mismatch = (coverage.baseline_matches == Some(false)).then(|| {
        format!(
            "coverage fingerprint mismatch: expected {}, observed {}",
            coverage.expected_fingerprint.as_deref().unwrap_or_default(),
            coverage.fingerprint
        )
    });
    (Some(coverage), mismatch)
}

fn coverage_for_files(
    files: Vec<CoverageFile>,
    source_lines: &std::collections::HashMap<String, std::collections::BTreeMap<u32, String>>,
) -> Option<CoverageFingerprint> {
    let files = merge_coverage_files(files);
    Some(CoverageFingerprint {
        fingerprint: coverage_hash_from_source_lines(&files, source_lines),
        files,
        expected_fingerprint: None,
        baseline_matches: None,
    })
}

fn compare_fingerprint(coverage: &mut CoverageFingerprint, expected: Option<&str>) {
    coverage.expected_fingerprint = expected.map(str::to_owned);
    coverage.baseline_matches = expected.map(|expected| expected == coverage.fingerprint);
}

pub fn load_dataset(path: &Path) -> Result<Dataset, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let value: Value = match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid dataset {}: {e}", path.display()))?,
        Some("yaml" | "yml") => serde_yaml::from_slice(&bytes)
            .map_err(|e| format!("invalid dataset {}: {e}", path.display()))?,
        _ => Err(format!(
            "dataset must use .json, .yaml, or .yml: {}",
            path.display()
        ))?,
    };
    validate_dataset_schema(&value)
        .map_err(|error| format!("invalid dataset {}: {error}", path.display()))?;
    serde_json::from_value(value).map_err(|e| format!("invalid dataset {}: {e}", path.display()))
}

fn validate_dataset_schema(value: &Value) -> Result<(), String> {
    DatasetSchema::iter_errors(value)
        .next()
        .map_or(Ok(()), |error| Err(error.to_string()))
}

#[derive(Clone)]
enum Check {
    Call(CallSpec),
    Sequence(SequenceSpec),
}

fn flatten_dataset_with_fingerprints(
    dataset: &Dataset,
    coverage_fingerprints: bool,
) -> Result<(Vec<ExecutionRequest>, Vec<Check>), String> {
    if dataset.version != 1 {
        return Err("dataset version must be 1".into());
    }
    let mut requests = Vec::new();
    let mut checks = Vec::new();
    let mut case_ids = std::collections::HashSet::new();
    for case in &dataset.cases {
        let case_id = match case {
            CaseSpec::Call(call) => &call.id,
            CaseSpec::Sequence(sequence) => &sequence.id,
        };
        if case_id.trim().is_empty() || !case_ids.insert(case_id.clone()) {
            return Err(format!(
                "case ID must be non-empty and unique: {:?}",
                case_id
            ));
        }
        match case {
            CaseSpec::Call(call) => {
                if coverage_fingerprints {
                    validate_fingerprint(&call.coverage_fingerprint, "case")?;
                }
                validate_setup(&call.setup)?;
                requests.push(ExecutionRequest {
                    key: ExecutionKey {
                        case_id: call.id.clone(),
                        step_id: None,
                    },
                    input: call.input.clone(),
                    setup: call.setup.clone(),
                    parallel: false,
                });
                checks.push(Check::Call(call.clone()));
            }
            CaseSpec::Sequence(sequence) => {
                if sequence.steps.is_empty() {
                    return Err(format!(
                        "sequence {} must include at least one step",
                        sequence.id
                    ));
                }
                let mut step_ids = std::collections::HashSet::new();
                for step in &sequence.steps {
                    if step.id.trim().is_empty() || !step_ids.insert(step.id.clone()) {
                        return Err(format!(
                            "step ID must be non-empty and unique in case {}: {:?}",
                            sequence.id, step.id
                        ));
                    }
                    if coverage_fingerprints {
                        validate_fingerprint(&step.coverage_fingerprint, "step")?;
                    }
                    validate_setup(&step.setup)?;
                    requests.push(ExecutionRequest {
                        key: ExecutionKey {
                            case_id: sequence.id.clone(),
                            step_id: Some(step.id.clone()),
                        },
                        input: step.input.clone(),
                        setup: step.setup.clone(),
                        parallel: step.parallel,
                    });
                }
                checks.push(Check::Sequence(sequence.clone()));
            }
        }
    }
    if requests.is_empty() {
        return Err("dataset must include at least one case or step".into());
    }
    Ok((requests, checks))
}

fn validate_setup(setup: &Option<String>) -> Result<(), String> {
    if setup
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err("setup must be a non-empty fixture ID when provided".into());
    }
    Ok(())
}

fn validate_fingerprint(value: &Option<String>, scope: &str) -> Result<(), String> {
    let value = value
        .as_deref()
        .ok_or_else(|| format!("coverage_fingerprint on {scope} is required"))?;
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "coverage_fingerprint on {scope} must be 32 lowercase hexadecimal characters"
        ));
    }
    if value.bytes().all(|byte| byte == b'0') {
        return Err(format!(
            "coverage_fingerprint on {scope} cannot represent zero coverage"
        ));
    }
    Ok(())
}

fn index_results(
    results: Vec<AdapterResult>,
) -> Result<std::collections::HashMap<ExecutionKey, AdapterResult>, String> {
    let mut indexed = std::collections::HashMap::new();
    for result in results {
        if indexed.insert(result.key.clone(), result).is_some() {
            return Err("adapter returned duplicate result keys".into());
        }
    }
    Ok(indexed)
}

fn verify_dataset(
    manifest: &Manifest,
    checks: Vec<Check>,
    mut outputs: std::collections::HashMap<ExecutionKey, AdapterResult>,
    coverage_scopes: &std::collections::HashMap<ExecutionKey, CoverageFingerprint>,
    fingerprint_source_lines: &std::collections::HashMap<
        String,
        std::collections::BTreeMap<u32, String>,
    >,
    coverage_fingerprints: bool,
    compare_fingerprints: bool,
) -> Result<Vec<ContractResult>, String> {
    let mut cases = Vec::new();
    for check in checks {
        match check {
            Check::Call(case) => {
                let key = ExecutionKey {
                    case_id: case.id.clone(),
                    step_id: None,
                };
                let result = outputs
                    .remove(&key)
                    .ok_or_else(|| format!("adapter omitted result for {}", case.id))?;
                let (passed, matched_by, error) =
                    evaluate_result(&result, case.expected.as_ref(), case.predicate.as_deref());
                let (coverage, coverage_error) = if coverage_fingerprints {
                    coverage_for_key(
                        coverage_scopes,
                        &key,
                        case.coverage_fingerprint.as_deref(),
                        compare_fingerprints,
                    )
                } else {
                    (None, None)
                };
                let coverage_error =
                    coverage_error.map(|error| format!("{}/{}: {error}", manifest.name, case.id));
                let passed = passed && coverage_error.is_none();
                cases.push(ContractResult {
                    contract: manifest.name.clone(),
                    case_id: case.id,
                    output: displayed_result(&result),
                    passed,
                    matched_by: if passed { matched_by } else { None },
                    error: error.or(coverage_error),
                    steps: vec![],
                    coverage,
                });
            }
            Check::Sequence(case) => {
                let mut step_results = Vec::new();
                let mut steps_passed = true;
                let mut first_error = None;
                let mut case_coverage_files = Vec::new();
                for step in &case.steps {
                    let key = ExecutionKey {
                        case_id: case.id.clone(),
                        step_id: Some(step.id.clone()),
                    };
                    let result = outputs.remove(&key).ok_or_else(|| {
                        format!("adapter omitted result for {}/{}", case.id, step.id)
                    })?;
                    let (passed, matched_by, error) =
                        evaluate_result(&result, step.expected.as_ref(), step.predicate.as_deref());
                    let (coverage, coverage_error) = if coverage_fingerprints {
                        coverage_for_key(
                            coverage_scopes,
                            &key,
                            step.coverage_fingerprint.as_deref(),
                            compare_fingerprints,
                        )
                    } else {
                        (None, None)
                    };
                    if let Some(observed) = &coverage {
                        case_coverage_files.extend(observed.files.clone());
                    }
                    let step_passed = passed && coverage_error.is_none();
                    steps_passed &= step_passed;
                    if first_error.is_none() {
                        first_error = error.clone().or(coverage_error.clone()).map(|message| {
                            format!("{}/{} step {}: {message}", manifest.name, case.id, step.id)
                        });
                    }
                    step_results.push(StepResult {
                        id: step.id.clone(),
                        output: displayed_result(&result),
                        passed: step_passed,
                        matched_by: if step_passed { matched_by } else { None },
                        error: error.or(coverage_error),
                        coverage,
                    });
                }
                let case_coverage =
                    coverage_for_files(case_coverage_files, fingerprint_source_lines);
                cases.push(ContractResult {
                    contract: manifest.name.clone(),
                    case_id: case.id,
                    output: None,
                    passed: steps_passed,
                    matched_by: None,
                    error: first_error,
                    steps: step_results,
                    coverage: case_coverage,
                });
            }
        }
    }
    if !outputs.is_empty() {
        return Err("adapter returned results that were not requested".into());
    }
    Ok(cases)
}

fn evaluate_result(
    result: &AdapterResult,
    expected: Option<&Value>,
    predicate: Option<&str>,
) -> (bool, Option<String>, Option<String>) {
    let Some(output) = displayed_result(result) else {
        return (
            false,
            None,
            Some("adapter returned neither output nor error".into()),
        );
    };
    match match_output(&output, expected, predicate) {
        Ok(MatchKind::Expected) => (true, Some("expected".into()), None),
        Ok(MatchKind::ExpectedAndPredicate) => (true, Some("expected+predicate".into()), None),
        Ok(MatchKind::Predicate) => (true, Some("predicate".into()), None),
        Err(error) => (false, None, Some(error)),
    }
}

fn displayed_result(result: &AdapterResult) -> Option<Value> {
    result
        .output
        .clone()
        .or_else(|| result.error.as_ref().map(|error| json!({"error": error})))
}

/// Contract-local JSON batch envelope. Paths remain relative to that contract.
pub fn batch_input(manifest: &Manifest, requests: &[ExecutionRequest]) -> Value {
    batch_input_with_fingerprints(manifest, requests, true)
}

/// Build the language-adapter request. Disabling per-entry fingerprints does
/// not disable aggregate dataset coverage collection.
pub fn batch_input_with_fingerprints(
    manifest: &Manifest,
    requests: &[ExecutionRequest],
    coverage_fingerprints: bool,
) -> Value {
    let mut coverage_sources = vec![manifest.files.validator.clone()];
    coverage_sources.extend(manifest.files.support.iter().cloned());
    coverage_sources.retain(|source| is_source_file(source));
    let mut input = json!({
        "contract": manifest.name,
        "package": ".",
        "validator": manifest.files.validator,
        "supplemental_tests": manifest.files.supplemental_tests,
        "coverage_sources": coverage_sources,
        "requests": requests,
    });
    if !coverage_fingerprints {
        input["coverage_fingerprints"] = Value::Bool(false);
    }
    input
}

#[cfg(test)]
#[path = "../tests/support/unit_tests.rs"]
mod tests;
