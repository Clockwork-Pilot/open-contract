use runner_isolation::{Backend, StagedTree};
use runner_manifest::{is_allowed, validate_package_path, Manifest, ManifestPlan};
use runner_verification::{match_output, MatchKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

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
    pub expected: Value,
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
    pub expected: Value,
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
}

/// Resolve the filesystem and command plan for a contract tree without
/// executing its declared commands.
pub fn show_isolation_plan(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    isolation_runtime: Option<PathBuf>,
) -> Result<String, String> {
    let root = ManifestPlan::load(manifest_path)?;
    let tree = StagedTree::create_with_runtime(&root, isolation_runtime)?;
    let mut output = String::new();

    let backend_name = match backend {
        Backend::None => "none",
        Backend::Bubblewrap => "bubblewrap",
        Backend::Landlock => "landlock",
    };
    output.push_str(&format!("Isolation: {backend_name}\n"));
    if backend == Backend::None {
        output.push_str("Filesystem enforcement: none (host access is unrestricted)\n");
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
                let input = batch_input(manifest, &batch.requests)
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
                    return Err(format!(
                        "contract {} execution.command failed ({}): {}",
                        manifest.name,
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                decode_batch_output(&output.stdout, &manifest.name)
            })
            .collect()
    }
}

fn decode_batch_output(bytes: &[u8], contract: &str) -> Result<AdapterBatch, String> {
    let response: AdapterBatch = serde_json::from_slice(bytes)
        .map_err(|e| format!("language adapter returned invalid batch JSON: {e}"))?;
    if response.results.is_empty() {
        return Err(format!("contract {contract} returned no batch results"));
    }
    Ok(response)
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
    let root = ManifestPlan::load(manifest_path)?;
    let tree = StagedTree::create_with_runtime(&root, isolation_runtime)?;
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
        let (requests, checks) = flatten_dataset(&dataset)?;
        batches.push(ContractBatch {
            manifest: manifest.clone(),
            package_prefix: package_prefix.to_path_buf(),
            requests,
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
        let coverage_scopes = index_coverage_scopes(batch, adapter_batch.coverage_scopes)?;
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
        let fingerprint = coverage_hash(&files);
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

fn coverage_hash(files: &[CoverageFile]) -> String {
    let encoded = coverage_files_json(files);
    let digest = Sha256::digest(encoded);
    format!("{:x}", digest)[..32].to_owned()
}

fn coverage_files_json(files: &[CoverageFile]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.push(b'[');
    for (file_index, file) in files.iter().enumerate() {
        if file_index > 0 {
            encoded.push(b',');
        }
        encoded.extend_from_slice(b"{\"path\":");
        push_json_string(&mut encoded, &file.path);
        encoded.extend_from_slice(b",\"lines\":[");
        for (line_index, line) in file.lines.iter().enumerate() {
            if line_index > 0 {
                encoded.push(b',');
            }
            encoded.extend_from_slice(line.to_string().as_bytes());
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
    compare_fingerprint(&mut coverage, expected);
    let mismatch = (coverage.baseline_matches == Some(false)).then(|| {
        format!(
            "coverage fingerprint mismatch: expected {}, observed {}",
            coverage.expected_fingerprint.as_deref().unwrap_or_default(),
            coverage.fingerprint
        )
    });
    (Some(coverage), mismatch)
}

fn coverage_for_files(files: Vec<CoverageFile>) -> Option<CoverageFingerprint> {
    let files = merge_coverage_files(files);
    Some(CoverageFingerprint {
        fingerprint: coverage_hash(&files),
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
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid dataset {}: {e}", path.display())),
        Some("yaml" | "yml") => serde_yaml::from_slice(&bytes)
            .map_err(|e| format!("invalid dataset {}: {e}", path.display())),
        _ => Err(format!(
            "dataset must use .json, .yaml, or .yml: {}",
            path.display()
        )),
    }
}

#[derive(Clone)]
enum Check {
    Call(CallSpec),
    Sequence(SequenceSpec),
}

fn flatten_dataset(dataset: &Dataset) -> Result<(Vec<ExecutionRequest>, Vec<Check>), String> {
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
                validate_fingerprint(&call.coverage_fingerprint, "case")?;
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
                    validate_fingerprint(&step.coverage_fingerprint, "step")?;
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
                    evaluate_result(&result, &case.expected, case.predicate.as_deref());
                let (coverage, coverage_error) =
                    coverage_for_key(coverage_scopes, &key, case.coverage_fingerprint.as_deref());
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
                        evaluate_result(&result, &step.expected, step.predicate.as_deref());
                    let (coverage, coverage_error) = coverage_for_key(
                        coverage_scopes,
                        &key,
                        step.coverage_fingerprint.as_deref(),
                    );
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
                let case_coverage = coverage_for_files(case_coverage_files);
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
    expected: &Value,
    predicate: Option<&str>,
) -> (bool, Option<String>, Option<String>) {
    let Some(output) = displayed_result(result) else {
        return (
            false,
            None,
            Some("adapter returned neither output nor error".into()),
        );
    };
    match match_output(&output, Some(expected), predicate) {
        Ok(MatchKind::Expected) => (true, Some("expected".into()), None),
        Ok(MatchKind::ExpectedAndPredicate) => (true, Some("expected+predicate".into()), None),
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
    let mut coverage_sources = vec![manifest.files.validator.clone()];
    coverage_sources.extend(manifest.files.support.iter().cloned());
    coverage_sources.retain(|source| is_source_file(source));
    json!({
        "contract": manifest.name,
        "package": ".",
        "validator": manifest.files.validator,
        "supplemental_tests": manifest.files.supplemental_tests,
        "coverage_sources": coverage_sources,
        "requests": requests,
    })
}

#[cfg(test)]
#[path = "../tests/support/unit_tests.rs"]
mod tests;
