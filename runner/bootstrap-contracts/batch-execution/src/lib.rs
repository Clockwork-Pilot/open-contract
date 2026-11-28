use runner_isolation::{Backend, StagedTree};
use runner_manifest::{is_allowed, validate_package_path, Manifest, ManifestPlan};
use runner_verification::{match_output, MatchKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
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
    /// Per-metric counts and file totals reported by the language coverage tool.
    #[serde(default)]
    pub coverage_metrics: std::collections::BTreeMap<String, CoverageMetricCounts>,
    /// Per-entity coverage locations supplied by the language adapter.
    #[serde(default)]
    pub coverage_entities: Vec<Value>,
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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CoverageMetricCounts {
    pub covered: u64,
    pub total: u64,
    #[serde(default)]
    pub files: Vec<CoverageMetricFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageMetricFile {
    pub path: String,
    pub covered: u64,
    pub total: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageMetricOutcome {
    pub status: String,
    pub covered: Option<u64>,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub minimum_percent: Option<f64>,
    pub threshold_met: Option<bool>,
    #[serde(default)]
    pub files: Vec<CoverageMetricFile>,
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
    pub dataset_path: PathBuf,
    pub requests: Vec<ExecutionRequest>,
    pub coverage_fingerprints: bool,
}

/// Whether a manifest filename contains the marker required for runner
/// execution. The marker must be a complete dot-delimited stem segment.
pub fn has_open_contract_filename(path: &Path) -> bool {
    let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = filename
        .strip_suffix(".yaml")
        .or_else(|| filename.strip_suffix(".yml"))
    else {
        return false;
    };
    stem.split('.').any(|segment| segment == "open-contract")
}

/// Resolve the filesystem and command plan for a contract tree without
/// executing its declared commands.
pub fn show_isolation_plan(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
) -> Result<String, String> {
    let root = ManifestPlan::load(manifest_path)?;
    let mut output = String::new();
    let tree_result = StagedTree::create_for_backend(&root, backend, isolation_runtime);

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
    write_backend_mounts(&mut output, backend);
    let (plans, traversal_diagnostics) = preview_contract_plans(&root);
    let mut diagnostics = traversal_diagnostics;
    let (read_paths, write_paths, display_root) = match &tree_result {
        Ok(tree) => (
            tree.read.clone(),
            tree.write.clone(),
            Some(tree.root.as_path()),
        ),
        Err(error) => {
            diagnostics.push(format!("filesystem staging is incomplete: {error}"));
            let (read, write) = declared_plan_paths(&plans);
            (read, write, None)
        }
    };
    output.push_str("Contract read-only paths:\n");
    write_plan_paths(&mut output, &read_paths, display_root);
    output.push_str("Contract writable paths:\n");
    write_plan_paths(&mut output, &write_paths, display_root);

    for (manifest, prefix) in &plans {
        output.push_str(&format!("Contract {}:\n", manifest.manifest.name));
        let execution = manifest.manifest.execution.as_ref();
        match (&tree_result, execution) {
            (Ok(tree), Some(execution)) => {
                let working_directory = tree.root.join(prefix);
                match tree.plan_command_at(backend, &working_directory, &execution.command) {
                    Ok(command) => {
                        write_command_plan(&mut output, &command);
                    }
                    Err(error) => {
                        output.push_str(&format!(
                            "  Working directory: {}\n",
                            working_directory.display()
                        ));
                        output.push_str(&format!("  Command argv: {:?}\n", execution.command));
                        output.push_str("  Runtime plan: unavailable\n");
                        diagnostics.push(format!(
                            "contract {} runtime plan is incomplete: {error}",
                            manifest.manifest.name
                        ));
                    }
                }
            }
            (Err(_), Some(execution)) => {
                output.push_str(&format!(
                    "  Working directory: {}\n",
                    manifest.package_root.join(prefix).display()
                ));
                output.push_str(&format!("  Command argv: {:?}\n", execution.command));
                output.push_str("  Runtime plan: unavailable (filesystem staging failed)\n");
            }
            (_, None) => {
                output.push_str(&format!(
                    "  Working directory: {}\n",
                    manifest.package_root.join(prefix).display()
                ));
                output.push_str("  Command: unavailable (execution.command is missing)\n");
                diagnostics.push(format!(
                    "contract {} is missing execution.command",
                    manifest.manifest.name
                ));
            }
        }
    }
    if !diagnostics.is_empty() {
        output.push_str("Plan diagnostics (some details may be unavailable):\n");
        for diagnostic in diagnostics {
            output.push_str(&format!("  Warning: {diagnostic}\n"));
        }
    }
    Ok(output)
}

fn write_command_plan(output: &mut String, command: &runner_isolation::StagedCommandPlan) {
    output.push_str(&format!(
        "  Working directory: {}\n",
        command.working_directory.display()
    ));
    output.push_str(&format!("  Command argv: {:?}\n", command.command));
    if let Some(executable) = &command.resolved_executable {
        output.push_str(&format!(
            "  Resolved executable: {}\n",
            executable.display()
        ));
    }
    output.push_str("  Runtime read-only paths:\n");
    for path in &command.runtime_roots {
        let kind = if path.is_dir() { "recursive" } else { "file" };
        output.push_str(&format!("    {} ({kind})\n", path.display()));
    }
}

fn write_backend_mounts(output: &mut String, backend: Backend) {
    if backend == Backend::Bubblewrap {
        output.push_str("Bubblewrap fixed mounts: /proc, /dev, /tmp, /contract\n");
        output.push_str("Bubblewrap contract root: /contract/<randomized-root>\n");
    }
}

fn preview_contract_plans(root: &ManifestPlan) -> (Vec<(ManifestPlan, PathBuf)>, Vec<String>) {
    let mut plans = vec![(root.clone(), PathBuf::new())];
    let mut diagnostics = Vec::new();
    let mut index = 0;
    while index < plans.len() {
        let (parent, _) = plans[index].clone();
        for nested in &parent.manifest.nested_contracts {
            match runner_manifest::resolve_nested_manifest_path(&parent.package_root, nested)
                .and_then(ManifestPlan::load)
            {
                Ok(child) => {
                    let child_prefix = child
                        .package_root
                        .strip_prefix(&root.package_root)
                        .unwrap_or(child.package_root.as_path())
                        .to_path_buf();
                    if !plans
                        .iter()
                        .any(|(existing, _)| existing.manifest_path == child.manifest_path)
                    {
                        plans.push((child, child_prefix));
                    }
                }
                Err(error) => diagnostics.push(format!(
                    "nested contract {nested:?} declared by {} could not be loaded: {error}",
                    parent.manifest.name
                )),
            }
        }
        index += 1;
    }
    (plans, diagnostics)
}

fn declared_plan_paths(plans: &[(ManifestPlan, PathBuf)]) -> (Vec<String>, Vec<String>) {
    let mut read = Vec::new();
    let mut write = Vec::new();
    for (manifest, prefix) in plans {
        for (entries, destination) in [
            (&manifest.manifest.filesystem.read, &mut read),
            (&manifest.manifest.filesystem.write, &mut write),
        ] {
            for raw in entries {
                let path = Path::new(raw);
                let display = if path.is_absolute() {
                    path.display().to_string()
                } else {
                    prefix.join(path).display().to_string()
                };
                if !destination.contains(&display) {
                    destination.push(display);
                }
            }
        }
    }
    (read, write)
}

fn write_plan_paths(output: &mut String, paths: &[String], display_root: Option<&Path>) {
    for path in paths {
        let candidate = display_root.map_or_else(|| PathBuf::from(path), |root| root.join(path));
        let kind = if candidate.is_dir() {
            "recursive"
        } else if candidate.is_file() {
            "file"
        } else if candidate.extension().is_some() {
            "file"
        } else {
            "recursive"
        };
        output.push_str(&format!("  {path} ({kind})\n"));
    }
}

/// Contract command contract: each command receives one complete flattened
/// request batch for all datasets declared by one contract and returns raw
/// outputs/errors and their union coverage. Dataset identity stays on each key
/// so Rust can verify each dataset's cases and fingerprints independently.
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
                let output =
                    tree.run_command_at(backend, &package_root, &execution.command, &input)?;
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
        "coverage_lines": value.get("coverage_lines").cloned().unwrap_or_else(|| json!([])),
        "coverage_metrics": value.get("coverage_metrics").cloned().unwrap_or_else(|| json!({})),
        "coverage_entities": value.get("coverage_entities").cloned().unwrap_or_else(|| json!([])),
        "additional_test_coverage": value.get("additional_test_coverage").cloned().unwrap_or(Value::Null),
        "additional_coverage_lines": value.get("additional_coverage_lines").cloned().unwrap_or_else(|| json!([])),
    });
    if let Err(error) = AdapterCoverageSchema::validate(&coverage) {
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
    // The JSON Schema validation above guarantees object shape, known metric
    // names, and the required metric and file fields. Keep the checks below
    // for cross-field invariants that JSON Schema cannot express.
    let metrics: std::collections::BTreeMap<String, CoverageMetricCounts> =
        serde_json::from_value(coverage["coverage_metrics"].clone()).map_err(|error| {
            format!("contract {contract} returned invalid adapter coverage metrics: {error}")
        })?;
    for (name, metric) in metrics {
        let covered = metric.covered;
        let total = metric.total;
        if covered > total {
            return Err(format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}: covered count exceeds total"));
        }
        let mut file_paths = std::collections::HashSet::new();
        let mut file_covered = 0_u64;
        let mut file_total = 0_u64;
        for (index, file) in metric.files.into_iter().enumerate() {
            let path = file.path;
            if !file_paths.insert(path.clone()) {
                return Err(format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}/files/{index}/path: duplicate file path {path:?}"));
            }
            let source_path = Path::new(&path);
            if path.is_empty()
                || source_path.is_absolute()
                || source_path
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}/files/{index}/path: path must be package-relative"));
            }
            let covered_count = file.covered;
            let total_count = file.total;
            if covered_count > total_count {
                return Err(format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}/files/{index}: covered count exceeds total"));
            }
            file_covered = file_covered.checked_add(covered_count).ok_or_else(|| {
                format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}/files: covered count overflow")
            })?;
            file_total = file_total.checked_add(total_count).ok_or_else(|| {
                format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}/files: total count overflow")
            })?;
        }
        if (file_covered, file_total) != (covered, total) {
            return Err(format!("contract {contract} returned invalid adapter coverage at /coverage_metrics/{name}: file counts do not add up to the metric totals"));
        }
    }
    Ok(())
}

fn adapter_coverage_value(batch: &AdapterBatch) -> Value {
    json!({
        "coverage": batch.coverage,
        "coverage_lines": batch.coverage_lines,
        "coverage_metrics": batch.coverage_metrics,
        "coverage_entities": batch.coverage_entities,
        "additional_test_coverage": batch.additional_test_coverage,
        "additional_coverage_lines": batch.additional_coverage_lines,
    })
}

fn coverage_metric_names() -> &'static [&'static str] {
    &[
        "line",
        "statement",
        "function",
        "branch",
        "region",
        "instantiation",
    ]
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
    #[serde(skip)]
    pub dataset_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageRecord {
    pub contract: String,
    #[serde(skip)]
    pub dataset_paths: Vec<PathBuf>,
    pub datasets: Vec<String>,
    pub minimum_coverage_percent: f64,
    pub dataset_covered_lines: usize,
    pub dataset_executable_lines: usize,
    pub combined_covered_lines: usize,
    pub combined_executable_lines: usize,
    pub line_threshold_met: bool,
    pub coverage_threshold_met: bool,
    pub dataset_metrics: std::collections::BTreeMap<String, CoverageMetricOutcome>,
    pub aggregate_metrics: std::collections::BTreeMap<String, CoverageMetricOutcome>,
    #[serde(default)]
    pub coverage_entities: Vec<Value>,
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
    run_contract_tree_with_fingerprint_mode_and_nested_contracts(
        manifest_path,
        backend,
        isolation_runtime,
        fingerprint_mode,
        true,
        adapter,
    )
}

/// Run a contract tree with explicit control over nested contract traversal.
/// When `include_nested_contracts` is false, only the root manifest is staged
/// and included in execution and coverage reports.
pub fn run_contract_tree_with_fingerprint_mode_and_nested_contracts(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
    fingerprint_mode: FingerprintMode,
    include_nested_contracts: bool,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    let coverage_fingerprints = fingerprint_mode != FingerprintMode::Disabled;
    let require_fingerprints = fingerprint_mode == FingerprintMode::Check;
    let compare_fingerprints = fingerprint_mode == FingerprintMode::Check;
    let root = ManifestPlan::load(manifest_path)?;
    if include_nested_contracts {
        validate_nested_manifest_filenames(&root)?;
    }
    let tree = StagedTree::create_for_backend_with_nested_contracts(
        &root,
        backend,
        isolation_runtime,
        include_nested_contracts,
    )?;
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
        for dataset_spec in &manifest.manifest.datasets {
            let dataset_name = dataset_spec.path();
            let data_path = manifest.package_root.join(dataset_name);
            let schema_path = dataset_spec
                .schema()
                .map(|path| manifest.package_root.join(path));
            let dataset = load_dataset_with_schema(&data_path, schema_path.as_deref())?;
            let (requests, checks) =
                flatten_dataset_with_fingerprints(&dataset, require_fingerprints)?;
            batches.push(ContractBatch {
                manifest: manifest.clone(),
                package_prefix: package_prefix.to_path_buf(),
                dataset_path: data_path,
                requests,
                coverage_fingerprints,
            });
            checks_by_contract.push(checks);
        }
    }
    // Execute every dataset for a contract in one adapter batch. Native metric
    // totals then represent the true union of locations reached by all those
    // datasets. Dataset identity is carried separately from case_id so the
    // validator still receives the user's original case id and fingerprints
    // remain independently keyed per dataset.
    let mut contract_batches: Vec<ContractBatch> = Vec::new();
    let mut dataset_indexes_by_contract: Vec<Vec<usize>> = Vec::new();
    let mut contract_index_by_name = std::collections::HashMap::new();
    for (dataset_index, dataset_batch) in batches.iter().enumerate() {
        let name = &dataset_batch.manifest.manifest.name;
        let contract_index = if let Some(index) = contract_index_by_name.get(name) {
            *index
        } else {
            let index = contract_batches.len();
            contract_index_by_name.insert(name.clone(), index);
            let mut combined = dataset_batch.clone();
            combined.requests.clear();
            contract_batches.push(combined);
            dataset_indexes_by_contract.push(Vec::new());
            index
        };
        let local_dataset_index = dataset_indexes_by_contract[contract_index].len();
        dataset_indexes_by_contract[contract_index].push(dataset_index);
        for request in &dataset_batch.requests {
            let mut request = request.clone();
            request.key.dataset_id = Some(local_dataset_index.to_string());
            contract_batches[contract_index].requests.push(request);
        }
    }
    let adapter_batches = adapter.execute_tree(&tree, backend, &contract_batches)?;
    if adapter_batches.len() != contract_batches.len() {
        return Err(format!(
            "adapter returned {} contract batches for {} requested packages",
            adapter_batches.len(),
            contract_batches.len()
        ));
    }
    let mut report = BatchReport::default();
    let mut checks_by_contract = checks_by_contract.into_iter().map(Some).collect::<Vec<_>>();
    for ((batch, dataset_indexes), mut adapter_batch) in contract_batches
        .iter()
        .zip(dataset_indexes_by_contract)
        .zip(adapter_batches)
    {
        let adapter_value = adapter_coverage_value(&adapter_batch);
        validate_adapter_coverage_payload(&adapter_value, &batch.manifest.manifest.name)?;
        validate_declared_coverage_sources(
            &adapter_batch,
            &batch.manifest.manifest.coverage_sources,
            &batch.manifest.manifest.name,
        )?;
        if let Some(line_metric) = adapter_batch.coverage_metrics.get("line") {
            let (covered, total) = line_coverage_totals(&adapter_batch.coverage_lines);
            if (line_metric.covered, line_metric.total) != (covered as u64, total as u64) {
                return Err(format!(
                    "contract {} reported line metric totals that do not match coverage_lines",
                    batch.manifest.manifest.name
                ));
            }
        }
        let additional_test_coverage = validate_additional_test_coverage(batch, &adapter_batch)?;
        prefix_line_coverage_paths(&batch.package_prefix, &mut adapter_batch.coverage_lines);
        prefix_line_coverage_paths(
            &batch.package_prefix,
            &mut adapter_batch.additional_coverage_lines,
        );
        prefix_coverage_metric_paths(&batch.package_prefix, &mut adapter_batch.coverage_metrics);
        prefix_coverage_entity_paths(&batch.package_prefix, &mut adapter_batch.coverage_entities);
        prefix_native_coverage_sources(&batch.package_prefix, &mut adapter_batch.coverage);
        let (dataset_covered_lines, dataset_executable_lines) =
            line_coverage_totals(&adapter_batch.coverage_lines);
        let combined_lines = merge_line_coverage(
            &adapter_batch.coverage_lines,
            &adapter_batch.additional_coverage_lines,
        );
        let (combined_covered_lines, combined_executable_lines) =
            line_coverage_totals(&combined_lines);
        let minimum_coverage_percent = batch.manifest.manifest.minimum_coverage_percent;
        let dataset_metrics = coverage_metric_outcomes(
            &adapter_batch.coverage_metrics,
            &adapter_batch.coverage_lines,
            minimum_coverage_percent,
        );
        let aggregate_metrics = coverage_metric_outcomes(
            &adapter_batch.coverage_metrics,
            &combined_lines,
            minimum_coverage_percent,
        );
        let line_threshold_met = aggregate_metrics
            .get("line")
            .and_then(|metric| metric.threshold_met)
            .unwrap_or(false);
        let coverage_threshold_met = aggregate_metrics
            .values()
            .all(|metric| metric.threshold_met != Some(false));
        let mut results_by_dataset = vec![Vec::new(); dataset_indexes.len()];
        let mut scopes_by_dataset = vec![Vec::new(); dataset_indexes.len()];
        for mut result in adapter_batch.results.drain(..) {
            let dataset_index = result
                .key
                .dataset_id
                .as_deref()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| {
                    format!(
                        "contract {} adapter result has no dataset identity",
                        batch.manifest.manifest.name
                    )
                })?;
            result.key.dataset_id = None;
            results_by_dataset
                .get_mut(dataset_index)
                .ok_or_else(|| {
                    format!(
                        "contract {} adapter result has invalid dataset identity",
                        batch.manifest.manifest.name
                    )
                })?
                .push(result);
        }
        for mut scope in adapter_batch.coverage_scopes.drain(..) {
            let dataset_index = scope
                .key
                .dataset_id
                .as_deref()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| {
                    format!(
                        "contract {} coverage scope has no dataset identity",
                        batch.manifest.manifest.name
                    )
                })?;
            scope.key.dataset_id = None;
            scopes_by_dataset
                .get_mut(dataset_index)
                .ok_or_else(|| {
                    format!(
                        "contract {} coverage scope has invalid dataset identity",
                        batch.manifest.manifest.name
                    )
                })?
                .push(scope);
        }
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
            dataset_paths: dataset_indexes
                .iter()
                .map(|index| batches[*index].dataset_path.clone())
                .collect(),
            datasets: dataset_indexes
                .iter()
                .map(|index| {
                    batches[*index]
                        .dataset_path
                        .strip_prefix(&batch.manifest.package_root)
                        .unwrap_or(&batches[*index].dataset_path)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
            minimum_coverage_percent,
            dataset_covered_lines,
            dataset_executable_lines,
            combined_covered_lines,
            combined_executable_lines,
            line_threshold_met,
            coverage_threshold_met,
            dataset_metrics,
            aggregate_metrics,
            coverage_entities: adapter_batch.coverage_entities,
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
        for (local_dataset_index, dataset_index) in dataset_indexes.into_iter().enumerate() {
            let dataset_batch = &batches[dataset_index];
            let outputs =
                index_results(std::mem::take(&mut results_by_dataset[local_dataset_index]))?;
            let mut fingerprint_source_lines = std::collections::HashMap::new();
            let coverage_scopes = if coverage_fingerprints {
                index_coverage_scopes(
                    dataset_batch,
                    std::mem::take(&mut scopes_by_dataset[local_dataset_index]),
                    &mut fingerprint_source_lines,
                )?
            } else {
                std::collections::HashMap::new()
            };
            let mut dataset_results = verify_dataset(
                &dataset_batch.manifest.manifest,
                checks_by_contract[dataset_index].take().unwrap_or_default(),
                outputs,
                &coverage_scopes,
                &fingerprint_source_lines,
                coverage_fingerprints,
                compare_fingerprints,
            )?;
            for result in &mut dataset_results {
                result.dataset_path = dataset_batch.dataset_path.clone();
            }
            report.results.extend(dataset_results);
        }
    }
    Ok(report)
}

/// Validate every nested manifest in a plan before the runner executes it.
pub fn validate_nested_manifest_filenames(root: &ManifestPlan) -> Result<(), String> {
    validate_nested_manifest_filenames_recursive(
        root,
        &mut Vec::new(),
        &mut std::collections::HashSet::new(),
    )
}

fn validate_nested_manifest_filenames_recursive(
    plan: &ManifestPlan,
    stack: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), String> {
    if stack.contains(&plan.manifest_path) {
        return Err(format!(
            "nested contract cycle detected: {}",
            plan.manifest_path.display()
        ));
    }
    if !visited.insert(plan.manifest_path.clone()) {
        return Ok(());
    }
    stack.push(plan.manifest_path.clone());
    for child_name in &plan.manifest.nested_contracts {
        let child_path =
            runner_manifest::resolve_nested_manifest_path(&plan.package_root, child_name)?;
        if !has_open_contract_filename(&child_path) {
            return Err(format!(
                "nested manifest filename must contain a dot-delimited open-contract segment before .yaml or .yml: {}",
                child_path.display()
            ));
        }
        let child = ManifestPlan::load(child_path)?;
        validate_nested_manifest_filenames_recursive(&child, stack, visited)?;
    }
    stack.pop();
    Ok(())
}

fn validate_declared_coverage_sources(
    batch: &AdapterBatch,
    declared_sources: &[String],
    contract: &str,
) -> Result<(), String> {
    for file in batch
        .coverage_lines
        .iter()
        .chain(batch.additional_coverage_lines.iter())
    {
        if !is_allowed(&file.path, declared_sources) {
            return Err(format!(
                "contract {contract} adapter reported coverage for source outside coverage_sources: {}",
                file.path
            ));
        }
    }
    for (metric, counts) in &batch.coverage_metrics {
        for file in &counts.files {
            if !is_allowed(&file.path, declared_sources) {
                return Err(format!(
                    "contract {contract} reported {metric} coverage for source outside coverage_sources: {}",
                    file.path
                ));
            }
        }
    }
    for entity in &batch.coverage_entities {
        let path = entity.get("path").and_then(Value::as_str).unwrap_or_default();
        if !is_allowed(path, declared_sources) {
            return Err(format!(
                "contract {contract} adapter reported entity coverage for source outside coverage_sources: {path}"
            ));
        }
    }
    if let Some(sources) = batch
        .coverage
        .as_ref()
        .and_then(|report| report.get("sources"))
        .and_then(Value::as_array)
    {
        for source in sources.iter().filter_map(Value::as_str) {
            if !is_allowed(source, declared_sources) {
                return Err(format!(
                    "contract {contract} adapter reported source outside coverage_sources: {source}"
                ));
            }
        }
    }
    Ok(())
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

fn prefix_coverage_metric_paths(
    prefix: &Path,
    metrics: &mut std::collections::BTreeMap<String, CoverageMetricCounts>,
) {
    if prefix.as_os_str().is_empty() || prefix == Path::new(".") {
        return;
    }
    for metric in metrics.values_mut() {
        for file in &mut metric.files {
            file.path = prefix.join(&file.path).to_string_lossy().replace('\\', "/");
        }
    }
}

fn prefix_coverage_entity_paths(prefix: &Path, entities: &mut [Value]) {
    if prefix.as_os_str().is_empty() || prefix == Path::new(".") {
        return;
    }
    for entity in entities {
        let path = entity["path"]
            .as_str()
            .expect("coverage entities are schema-validated before path prefixing");
        entity["path"] = Value::String(prefix.join(path).to_string_lossy().replace('\\', "/"));
    }
}

fn declared_sources(batch: &ContractBatch) -> Vec<String> {
    batch
        .manifest
        .manifest
        .coverage_sources
        .iter()
        .map(|source| {
            batch
                .package_prefix
                .join(source)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn validate_additional_test_coverage(
    batch: &ContractBatch,
    adapter_batch: &AdapterBatch,
) -> Result<Value, String> {
    let test_files = &batch.manifest.manifest.supplemental_tests;
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

fn coverage_metric_outcomes(
    reported: &std::collections::BTreeMap<String, CoverageMetricCounts>,
    line_files: &[CoverageLineFile],
    minimum_coverage_percent: f64,
) -> std::collections::BTreeMap<String, CoverageMetricOutcome> {
    let (line_covered, line_total) = line_coverage_totals(line_files);
    let mut metrics = reported.clone();
    metrics.insert(
        "line".into(),
        CoverageMetricCounts {
            covered: line_covered as u64,
            total: line_total as u64,
            files: line_files
                .iter()
                .map(|file| CoverageMetricFile {
                    path: file.path.clone(),
                    covered: file
                        .covered_lines
                        .iter()
                        .copied()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len() as u64,
                    total: file
                        .executable_lines
                        .iter()
                        .copied()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len() as u64,
                })
                .collect(),
        },
    );
    coverage_metric_names()
        .iter()
        .map(|name| {
            let outcome = if let Some(counts) = metrics.get(*name) {
                let minimum_percent = minimum_coverage_percent;
                let percent = if counts.total == 0 {
                    0.0
                } else {
                    counts.covered as f64 * 100.0 / counts.total as f64
                };
                CoverageMetricOutcome {
                    status: "available".into(),
                    covered: Some(counts.covered),
                    total: Some(counts.total),
                    percent: Some(percent),
                    minimum_percent: Some(minimum_percent),
                    threshold_met: Some(
                        counts.total > 0 && percent + f64::EPSILON >= minimum_percent,
                    ),
                    files: counts.files.clone(),
                }
            } else {
                CoverageMetricOutcome {
                    status: "unavailable".into(),
                    covered: None,
                    total: None,
                    percent: None,
                    minimum_percent: None,
                    threshold_met: None,
                    files: Vec::new(),
                }
            };
            ((*name).to_owned(), outcome)
        })
        .collect()
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
    let allowed_sources = &batch.manifest.manifest.coverage_sources;
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
            if !is_allowed(&file.path, allowed_sources) {
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
            expected.map(|_| {
                format!(
                    "coverage fingerprint unavailable for {}",
                    execution_key_debug(key)
                )
            }),
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

fn execution_key_debug(key: &ExecutionKey) -> String {
    match &key.step_id {
        Some(step_id) => format!(
            "ExecutionKey {{ case_id: {:?}, step_id: Some({:?}) }}",
            key.case_id, step_id
        ),
        None => format!(
            "ExecutionKey {{ case_id: {:?}, step_id: None }}",
            key.case_id
        ),
    }
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
    load_dataset_with_schema(path, None)
}

pub fn load_dataset_with_schema(
    path: &Path,
    schema_path: Option<&Path>,
) -> Result<Dataset, String> {
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
    let dataset: Dataset = serde_json::from_value(value)
        .map_err(|e| format!("invalid dataset {}: {e}", path.display()))?;
    let schema = schema_path
        .map(load_value_schema)
        .transpose()
        .map_err(|error| {
            format!(
                "invalid value schema for dataset {}: {error}",
                path.display()
            )
        })?;
    validate_dataset_values(&dataset, schema.as_ref())
        .map_err(|error| format!("invalid dataset {}: {error}", path.display()))?;
    Ok(dataset)
}

#[derive(Clone, Debug)]
struct DatasetValueSchemas {
    input: Option<ValueSchema>,
    expected: Option<ValueSchema>,
}

#[derive(Clone, Debug)]
struct ValueSchema {
    kind: ValueSchemaKind,
    allowed: Option<Vec<Value>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueSchemaType {
    String,
    Integer,
    Number,
    Boolean,
    Null,
    Object,
    Array,
}

#[derive(Clone, Debug)]
enum ValueSchemaKind {
    String {
        min: Option<u64>,
        max: Option<u64>,
    },
    Integer {
        min: Option<f64>,
        max: Option<f64>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
    },
    Boolean,
    Null,
    Object {
        fields: std::collections::BTreeMap<String, ValueSchema>,
        required: Vec<String>,
        additional: bool,
    },
    Array {
        items: Box<ValueSchema>,
        min: Option<u64>,
        max: Option<u64>,
    },
}

fn load_value_schema(path: &Path) -> Result<DatasetValueSchemas, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let yaml: serde_yaml::Value = serde_yaml::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    let document = serde_json::to_value(yaml)
        .map_err(|error| format!("cannot normalize {}: {error}", path.display()))?;
    let definitions = document.as_object().ok_or_else(|| {
        "schema document must be a mapping with input and/or expected".to_string()
    })?;
    if definitions.is_empty()
        || definitions
            .keys()
            .any(|key| key != "input" && key != "expected")
    {
        return Err("schema document may contain only input and expected definitions".into());
    }
    let input = definitions
        .get("input")
        .map(|value| parse_value_schema(value, "input"))
        .transpose()?;
    let expected = definitions
        .get("expected")
        .map(|value| parse_value_schema(value, "expected"))
        .transpose()?;
    Ok(DatasetValueSchemas { input, expected })
}

fn parse_value_schema(value: &Value, location: &str) -> Result<ValueSchema, String> {
    if let Some(type_name) = value.as_str() {
        return parse_simple_schema(type_name, location);
    }
    let object = value
        .as_object()
        .ok_or_else(|| format!("{location}: schema must be a type name or mapping"))?;
    let type_name = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{location}: schema mapping requires a string type"))?;
    let schema_type = match type_name {
        "string" => ValueSchemaType::String,
        "integer" => ValueSchemaType::Integer,
        "number" => ValueSchemaType::Number,
        "boolean" => ValueSchemaType::Boolean,
        "null" => ValueSchemaType::Null,
        "object" => ValueSchemaType::Object,
        "array" => ValueSchemaType::Array,
        _ => return Err(format!("{location}: unsupported type {type_name:?}")),
    };
    let common = ["type", "enum"];
    let type_keys: &[&str] = match schema_type {
        ValueSchemaType::String => &["min_length", "max_length"],
        ValueSchemaType::Integer | ValueSchemaType::Number => &["minimum", "maximum"],
        ValueSchemaType::Boolean | ValueSchemaType::Null => &[],
        ValueSchemaType::Object => &["required", "fields", "additional"],
        ValueSchemaType::Array => &["items", "min_items", "max_items"],
    };
    for key in object.keys() {
        if !common.contains(&key.as_str()) && !type_keys.contains(&key.as_str()) {
            return Err(format!(
                "{location}: unsupported schema keyword {key:?} for {type_name}"
            ));
        }
    }
    let allowed = parse_enum(object.get("enum"), location)?;
    let kind = match schema_type {
        ValueSchemaType::String => {
            let min = parse_u64_bound(object.get("min_length"), "min_length", location)?;
            let max = parse_u64_bound(object.get("max_length"), "max_length", location)?;
            check_bounds(min, max, "string length", location)?;
            ValueSchemaKind::String { min, max }
        }
        ValueSchemaType::Integer | ValueSchemaType::Number => {
            let min = parse_number_bound(object.get("minimum"), "minimum", location)?;
            let max = parse_number_bound(object.get("maximum"), "maximum", location)?;
            check_numeric_bounds(min, max, location)?;
            if schema_type == ValueSchemaType::Integer {
                ValueSchemaKind::Integer { min, max }
            } else {
                ValueSchemaKind::Number { min, max }
            }
        }
        ValueSchemaType::Boolean => ValueSchemaKind::Boolean,
        ValueSchemaType::Null => ValueSchemaKind::Null,
        ValueSchemaType::Object => {
            let raw_fields = object
                .get("fields")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{location}: object schema requires fields mapping"))?;
            let mut fields = std::collections::BTreeMap::new();
            for (name, field) in raw_fields {
                fields.insert(
                    name.clone(),
                    parse_value_schema(field, &format!("{location}.fields.{name}"))?,
                );
            }
            let mut required = Vec::new();
            if let Some(raw_required) = object.get("required") {
                let entries = raw_required
                    .as_array()
                    .ok_or_else(|| format!("{location}.required: expected a list"))?;
                for entry in entries {
                    let name = entry
                        .as_str()
                        .ok_or_else(|| format!("{location}.required: entries must be strings"))?;
                    if !fields.contains_key(name) {
                        return Err(format!(
                            "{location}.required: {name:?} is not declared in fields"
                        ));
                    }
                    required.push(name.to_owned());
                }
            }
            let additional = object
                .get("additional")
                .map(|value| {
                    value
                        .as_bool()
                        .ok_or_else(|| format!("{location}.additional: expected a boolean"))
                })
                .transpose()?
                .unwrap_or(true);
            ValueSchemaKind::Object {
                fields,
                required,
                additional,
            }
        }
        ValueSchemaType::Array => {
            let raw_items = object
                .get("items")
                .ok_or_else(|| format!("{location}: array schema requires items"))?;
            let items = Box::new(parse_value_schema(raw_items, &format!("{location}.items"))?);
            let min = parse_u64_bound(object.get("min_items"), "min_items", location)?;
            let max = parse_u64_bound(object.get("max_items"), "max_items", location)?;
            check_bounds(min, max, "array length", location)?;
            ValueSchemaKind::Array { items, min, max }
        }
    };
    Ok(ValueSchema { kind, allowed })
}

fn parse_simple_schema(type_name: &str, location: &str) -> Result<ValueSchema, String> {
    let kind = match type_name {
        "string" => ValueSchemaKind::String {
            min: None,
            max: None,
        },
        "integer" => ValueSchemaKind::Integer {
            min: None,
            max: None,
        },
        "number" => ValueSchemaKind::Number {
            min: None,
            max: None,
        },
        "boolean" => ValueSchemaKind::Boolean,
        "null" => ValueSchemaKind::Null,
        "object" => ValueSchemaKind::Object {
            fields: Default::default(),
            required: Vec::new(),
            additional: true,
        },
        "array" => return Err(format!("{location}: array type requires an items schema")),
        _ => return Err(format!("{location}: unsupported type {type_name:?}")),
    };
    Ok(ValueSchema {
        kind,
        allowed: None,
    })
}

fn parse_enum(value: Option<&Value>, location: &str) -> Result<Option<Vec<Value>>, String> {
    value
        .map(|value| {
            let values = value
                .as_array()
                .filter(|values| !values.is_empty())
                .ok_or_else(|| format!("{location}.enum: expected a non-empty list"))?;
            Ok(values.clone())
        })
        .transpose()
}

fn parse_u64_bound(
    value: Option<&Value>,
    key: &str,
    location: &str,
) -> Result<Option<u64>, String> {
    value
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| format!("{location}.{key}: expected a non-negative integer"))
        })
        .transpose()
}

fn parse_number_bound(
    value: Option<&Value>,
    key: &str,
    location: &str,
) -> Result<Option<f64>, String> {
    value
        .map(|value| {
            value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("{location}.{key}: expected a finite number"))
        })
        .transpose()
}

fn check_bounds(
    min: Option<u64>,
    max: Option<u64>,
    label: &str,
    location: &str,
) -> Result<(), String> {
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return Err(format!("{location}: minimum {label} exceeds maximum"));
    }
    Ok(())
}

fn check_numeric_bounds(min: Option<f64>, max: Option<f64>, location: &str) -> Result<(), String> {
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return Err(format!("{location}: minimum exceeds maximum"));
    }
    Ok(())
}

fn validate_dataset_values(
    dataset: &Dataset,
    schema: Option<&DatasetValueSchemas>,
) -> Result<(), String> {
    for case in &dataset.cases {
        match case {
            CaseSpec::Call(call) => {
                validate_call_values(&call.id, &call.input, call.expected.as_ref(), schema)?
            }
            CaseSpec::Sequence(sequence) => {
                for step in &sequence.steps {
                    validate_call_values(
                        &format!("{} step {}", sequence.id, step.id),
                        &step.input,
                        step.expected.as_ref(),
                        schema,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_call_values(
    identity: &str,
    input: &Value,
    expected: Option<&Value>,
    schema: Option<&DatasetValueSchemas>,
) -> Result<(), String> {
    if let Some(input_schema) = schema.and_then(|schema| schema.input.as_ref()) {
        validate_schema_value(input, input_schema, "$")
            .map_err(|error| format!("case/step {identity:?} input {error}"))?;
    }
    if let (Some(value), Some(expected_schema)) =
        (expected, schema.and_then(|schema| schema.expected.as_ref()))
    {
        validate_schema_value(value, expected_schema, "$")
            .map_err(|error| format!("case/step {identity:?} expected {error}"))?;
    }
    Ok(())
}

fn validate_schema_value(
    value: &Value,
    schema: &ValueSchema,
    location: &str,
) -> Result<(), String> {
    if schema
        .allowed
        .as_ref()
        .is_some_and(|values| !values.contains(value))
    {
        return Err(format!("at {location}: value is not in enum"));
    }
    match &schema.kind {
        ValueSchemaKind::String { min, max } => {
            let text = value
                .as_str()
                .ok_or_else(|| format!("at {location}: expected string"))?;
            validate_length(text.chars().count(), *min, *max, location)?;
        }
        ValueSchemaKind::Integer { min, max } => {
            if value.as_i64().is_none() && value.as_u64().is_none() {
                return Err(format!("at {location}: expected integer"));
            }
            validate_number(value, *min, *max, location)?;
        }
        ValueSchemaKind::Number { min, max } => validate_number(value, *min, *max, location)?,
        ValueSchemaKind::Boolean => {
            if !value.is_boolean() {
                return Err(format!("at {location}: expected boolean"));
            }
        }
        ValueSchemaKind::Null => {
            if !value.is_null() {
                return Err(format!("at {location}: expected null"));
            }
        }
        ValueSchemaKind::Object {
            fields,
            required,
            additional,
        } => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("at {location}: expected object"))?;
            for name in required {
                if !object.contains_key(name) {
                    return Err(format!("at {location}.{name}: required field is missing"));
                }
            }
            for (name, field_value) in object {
                if let Some(field_schema) = fields.get(name) {
                    validate_schema_value(
                        field_value,
                        field_schema,
                        &format!("{location}.{name}"),
                    )?;
                } else if !additional {
                    return Err(format!("at {location}.{name}: unknown field"));
                }
            }
        }
        ValueSchemaKind::Array { items, min, max } => {
            let entries = value
                .as_array()
                .ok_or_else(|| format!("at {location}: expected array"))?;
            for (index, entry) in entries.iter().enumerate() {
                validate_schema_value(entry, items, &format!("{location}[{index}]"))?;
            }
            validate_length(entries.len(), *min, *max, location)?;
        }
    }
    Ok(())
}

fn validate_number(
    value: &Value,
    min: Option<f64>,
    max: Option<f64>,
    location: &str,
) -> Result<(), String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("at {location}: expected number"))?;
    if min.is_some_and(|bound| number < bound) {
        return Err(format!("at {location}: value is below minimum"));
    }
    if max.is_some_and(|bound| number > bound) {
        return Err(format!("at {location}: value is above maximum"));
    }
    Ok(())
}

fn validate_length(
    length: usize,
    min: Option<u64>,
    max: Option<u64>,
    location: &str,
) -> Result<(), String> {
    if min.is_some_and(|bound| (length as u64) < bound) {
        return Err(format!("at {location}: length is below minimum"));
    }
    if max.is_some_and(|bound| (length as u64) > bound) {
        return Err(format!("at {location}: length is above maximum"));
    }
    Ok(())
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
                        dataset_id: None,
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
                            dataset_id: None,
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
                    dataset_id: None,
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
                    dataset_path: PathBuf::new(),
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
                        dataset_id: None,
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
                    dataset_path: PathBuf::new(),
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
    let mut input = json!({
        "contract": manifest.name,
        "package": ".",
        "supplemental_tests": manifest.supplemental_tests,
        "coverage_sources": manifest.coverage_sources,
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
