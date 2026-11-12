use crate::isolation::{Backend, StagedTree};
use crate::manifest::{Manifest, ManifestPlan};
use crate::verify::{match_output, MatchKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub version: u32,
    pub kind: Option<String>,
    pub config: Option<Value>,
    pub cases: Option<Vec<CaseSpec>>,
    pub targets: Option<Vec<TargetSpec>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    pub id: String,
    pub kind: Option<String>,
    pub config: Option<Value>,
    #[serde(default)]
    pub cases: Vec<CaseSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseSpec {
    pub id: String,
    pub input: Value,
    pub expected: Option<Value>,
    pub predicate: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub id: String,
    pub input: Value,
    pub expected: Option<Value>,
    pub predicate: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct ExecutionKey {
    pub target_id: String,
    pub case_id: String,
    pub step_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub key: ExecutionKey,
    pub kind: Option<String>,
    pub config: Option<Value>,
    pub input: Value,
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
}

#[derive(Clone, Debug)]
pub struct ContractBatch {
    pub manifest: ManifestPlan,
    pub package_prefix: PathBuf,
    pub requests: Vec<ExecutionRequest>,
}

/// Adapter contract: one invocation for every package in the root verification
/// tree. Language adapters return raw outputs/errors and native coverage; they
/// do not compare expectations or evaluate predicates.
pub trait BatchAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String>;
}

/// Runs one custom tree adapter command inside the root isolation. It receives
/// one JSON document containing all parent and nested package batches.
pub struct CommandBatchAdapter {
    pub command: Vec<String>,
    pub runtime_profile: Option<String>,
}

/// Starts one Python coordinator in the root isolation. It dispatches Python
/// or JavaScript batch runners inside that already established boundary.
pub struct RuntimeBatchAdapter;

impl BatchAdapter for RuntimeBatchAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        for batch in batches {
            let suffix = Path::new(&batch.manifest.manifest.files.validator)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if !matches!(suffix, "py" | "js" | "mjs") {
                return Err(format!(
                    "no built-in runtime for validator extension .{suffix}"
                ));
            }
        }
        let command = vec![
            find_executable("python3")?,
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../python/src/open_contracts_runner/tree_batch_adapter.py")
                .to_string_lossy()
                .into_owned(),
        ];
        let input = serde_json::to_vec(&tree_batch_input(batches)).map_err(|e| e.to_string())?;
        let output = tree.run_command(backend, &tree.root, &command, &input, Some("all"))?;
        if !output.status.success() {
            return Err(format!(
                "language adapter coordinator failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        decode_tree_output(&output.stdout, batches)
    }
}

fn find_executable(name: &str) -> Result<String, String> {
    let path = std::env::var_os("PATH").ok_or_else(|| "PATH is not set".to_string())?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(if candidate.is_absolute() {
                candidate.to_string_lossy().into_owned()
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(candidate)
                    .to_string_lossy()
                    .into_owned()
            });
        }
    }
    Err(format!("required runtime executable not found: {name}"))
}

impl BatchAdapter for CommandBatchAdapter {
    fn execute_tree(
        &self,
        tree: &StagedTree,
        backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        let input = serde_json::to_vec(&tree_batch_input(batches)).map_err(|e| e.to_string())?;
        let output = tree.run_command(
            backend,
            &tree.root,
            &self.command,
            &input,
            self.runtime_profile.as_deref(),
        )?;
        if !output.status.success() {
            return Err(format!(
                "language adapter failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        decode_tree_output(&output.stdout, batches)
    }
}

#[derive(Deserialize)]
struct NamedBatch {
    contract: String,
    #[serde(flatten)]
    batch: AdapterBatch,
}

#[derive(Deserialize)]
struct TreeBatchOutput {
    contracts: Vec<NamedBatch>,
}

fn decode_tree_output(
    bytes: &[u8],
    batches: &[ContractBatch],
) -> Result<Vec<AdapterBatch>, String> {
    let response: TreeBatchOutput = serde_json::from_slice(bytes)
        .map_err(|e| format!("language adapter returned invalid batch JSON: {e}"))?;
    let mut by_name = std::collections::HashMap::new();
    for item in response.contracts {
        if by_name.insert(item.contract.clone(), item.batch).is_some() {
            return Err(format!(
                "language adapter returned duplicate contract batch: {}",
                item.contract
            ));
        }
    }
    batches
        .iter()
        .map(|batch| {
            by_name
                .remove(&batch.manifest.manifest.name)
                .ok_or_else(|| {
                    format!(
                        "language adapter omitted contract batch: {}",
                        batch.manifest.manifest.name
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|ordered| {
            if by_name.is_empty() {
                Ok(ordered)
            } else {
                Err("language adapter returned an unknown contract batch".into())
            }
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    pub output: Option<Value>,
    pub passed: bool,
    pub matched_by: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractResult {
    pub contract: String,
    pub target_id: String,
    pub case_id: String,
    pub output: Option<Value>,
    pub passed: bool,
    pub matched_by: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageRecord {
    pub contract: String,
    pub native_report: Value,
    pub declared_sources: Vec<String>,
    pub covered_sources: Vec<String>,
    pub missing_sources: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BatchReport {
    pub results: Vec<ContractResult>,
    /// Raw native reports from the root and every child run in this shared
    /// isolation. Adapters own merging reports whose formats are language-specific.
    pub coverage: Vec<CoverageRecord>,
}

impl BatchReport {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }
    pub fn total(&self) -> usize {
        self.results.len()
    }
    pub fn coverage_complete(&self) -> bool {
        !self.coverage.is_empty()
            && self
                .coverage
                .iter()
                .all(|record| record.missing_sources.is_empty())
    }
}

pub fn run_contract_tree(
    manifest_path: impl AsRef<Path>,
    backend: Backend,
    adapter: &dyn BatchAdapter,
) -> Result<BatchReport, String> {
    let root = ManifestPlan::load(manifest_path)?;
    let tree = StagedTree::create(&root)?;
    let mut batches = Vec::new();
    let mut checks_by_contract = Vec::new();
    let mut names = std::collections::HashSet::new();
    for manifest in &tree.manifests {
        if !names.insert(manifest.manifest.name.clone()) {
            return Err(format!(
                "duplicate contract name in nested tree: {}",
                manifest.manifest.name
            ));
        }
        let package_prefix = manifest
            .package_root
            .strip_prefix(&root.package_root)
            .map_err(|e| e.to_string())?;
        let package_prefix = if package_prefix == Path::new("") {
            PathBuf::new()
        } else {
            package_prefix.to_path_buf()
        };
        let data_path = manifest.package_root.join(&manifest.manifest.files.dataset);
        let dataset = load_dataset(&data_path)?;
        let (requests, checks) = flatten_dataset(&dataset)?;
        batches.push(ContractBatch {
            manifest: manifest.clone(),
            package_prefix,
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
    for ((batch, checks), adapter_batch) in
        batches.iter().zip(checks_by_contract).zip(adapter_batches)
    {
        let outputs = index_results(adapter_batch.results)?;
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
            native_report: adapter_batch.coverage.unwrap_or(Value::Null),
            declared_sources,
            covered_sources: {
                let mut sources = covered_sources.into_iter().collect::<Vec<_>>();
                sources.sort();
                sources
            },
            missing_sources,
        });
        report
            .results
            .extend(verify_dataset(&batch.manifest.manifest, checks, outputs)?);
    }
    Ok(report)
}

fn declared_sources(batch: &ContractBatch) -> Vec<String> {
    let mut sources = vec![batch.manifest.manifest.files.validator.clone()];
    sources.extend(batch.manifest.manifest.files.support.iter().cloned());
    sources
        .into_iter()
        .map(|source| {
            batch
                .package_prefix
                .join(source)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn load_dataset(path: &Path) -> Result<Dataset, String> {
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
    Case { target: String, case: CaseSpec },
    Sequence { target: String, case: CaseSpec },
}

fn flatten_dataset(dataset: &Dataset) -> Result<(Vec<ExecutionRequest>, Vec<Check>), String> {
    if dataset.version != 1 {
        return Err("dataset version must be 1".into());
    }
    let mut requests = Vec::new();
    let mut checks = Vec::new();
    let targets = match (&dataset.cases, &dataset.targets) {
        (Some(cases), None) => vec![TargetSpec {
            id: "default".into(),
            kind: dataset.kind.clone(),
            config: dataset.config.clone(),
            cases: cases.clone(),
        }],
        (None, Some(targets)) => targets.clone(),
        _ => return Err("dataset must contain exactly one of cases or targets".into()),
    };
    let mut target_ids = std::collections::HashSet::new();
    for target in targets {
        if target.id.trim().is_empty() || !target_ids.insert(target.id.clone()) {
            return Err(format!(
                "target ID must be non-empty and unique: {:?}",
                target.id
            ));
        }
        let mut case_ids = std::collections::HashSet::new();
        for case in target.cases {
            if case.id.trim().is_empty() || !case_ids.insert(case.id.clone()) {
                return Err(format!(
                    "case ID must be non-empty and unique in target {}: {:?}",
                    target.id, case.id
                ));
            }
            if case.expected.is_some() == case.predicate.is_some() {
                return Err(format!(
                    "case {}/{} must contain exactly one of expected or predicate",
                    target.id, case.id
                ));
            }
            if case.steps.is_empty() {
                requests.push(ExecutionRequest {
                    key: ExecutionKey {
                        target_id: target.id.clone(),
                        case_id: case.id.clone(),
                        step_id: None,
                    },
                    kind: target.kind.clone(),
                    config: target.config.clone(),
                    input: case.input.clone(),
                });
                checks.push(Check::Case {
                    target: target.id.clone(),
                    case,
                });
            } else {
                let mut step_ids = std::collections::HashSet::new();
                for step in &case.steps {
                    if step.id.trim().is_empty() || !step_ids.insert(step.id.clone()) {
                        return Err(format!(
                            "step ID must be non-empty and unique in case {}/{}: {:?}",
                            target.id, case.id, step.id
                        ));
                    }
                    if step.expected.is_some() && step.predicate.is_some() {
                        return Err(format!(
                            "step {}/{}/{} cannot contain both expected and predicate",
                            target.id, case.id, step.id
                        ));
                    }
                    requests.push(ExecutionRequest {
                        key: ExecutionKey {
                            target_id: target.id.clone(),
                            case_id: case.id.clone(),
                            step_id: Some(step.id.clone()),
                        },
                        kind: target.kind.clone(),
                        config: target.config.clone(),
                        input: step.input.clone(),
                    });
                }
                checks.push(Check::Sequence {
                    target: target.id.clone(),
                    case,
                });
            }
        }
    }
    if requests.is_empty() {
        return Err("dataset must include at least one case or step".into());
    }
    Ok((requests, checks))
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
) -> Result<Vec<ContractResult>, String> {
    let mut cases = Vec::new();
    for check in checks {
        match check {
            Check::Case { target, case } => {
                let key = ExecutionKey {
                    target_id: target.clone(),
                    case_id: case.id.clone(),
                    step_id: None,
                };
                let result = outputs
                    .remove(&key)
                    .ok_or_else(|| format!("adapter omitted result for {}/{}", target, case.id))?;
                let (passed, matched_by, error) =
                    evaluate_result(&result, case.expected.as_ref(), case.predicate.as_deref());
                cases.push(ContractResult {
                    contract: manifest.name.clone(),
                    target_id: target,
                    case_id: case.id,
                    output: result.output,
                    passed,
                    matched_by,
                    error,
                    steps: vec![],
                });
            }
            Check::Sequence { target, case } => {
                let mut step_outputs = serde_json::Map::new();
                let mut step_checks = Vec::new();
                let mut step_results = Vec::new();
                let mut steps_passed = true;
                let mut first_error = None;
                for step in &case.steps {
                    let key = ExecutionKey {
                        target_id: target.clone(),
                        case_id: case.id.clone(),
                        step_id: Some(step.id.clone()),
                    };
                    let result = outputs.remove(&key).ok_or_else(|| {
                        format!(
                            "adapter omitted result for {}/{}/{}",
                            target, case.id, step.id
                        )
                    })?;
                    let (passed, matched_by, error) = if step.expected.is_none()
                        && step.predicate.is_none()
                    {
                        (result.error.is_none(), None, result.error.clone())
                    } else {
                        evaluate_result(&result, step.expected.as_ref(), step.predicate.as_deref())
                    };
                    steps_passed &= passed;
                    if first_error.is_none() {
                        first_error = error.clone();
                    }
                    step_checks.push(json!({
                        "id": step.id,
                        "passed": passed,
                        "matched_by": matched_by,
                    }));
                    step_outputs.insert(
                        step.id.clone(),
                        result.output.clone().unwrap_or(Value::Null),
                    );
                    step_results.push(StepResult {
                        id: step.id.clone(),
                        output: result.output,
                        passed,
                        matched_by,
                        error,
                    });
                }
                let actual = json!({
                    "input": case.input,
                    "steps": step_outputs,
                    "captures": {},
                    "checks": step_checks,
                });
                let case_check =
                    match_output(&actual, case.expected.as_ref(), case.predicate.as_deref());
                let (case_passed, matched_by, case_error) = match case_check {
                    Ok(MatchKind::Exact) => (true, Some("exact".into()), None),
                    Ok(MatchKind::Predicate) => (true, Some("predicate".into()), None),
                    Err(error) => (false, None, Some(error)),
                };
                let passed = steps_passed && case_passed;
                cases.push(ContractResult {
                    contract: manifest.name.clone(),
                    target_id: target,
                    case_id: case.id,
                    output: Some(actual),
                    passed,
                    matched_by: if passed { matched_by } else { None },
                    error: first_error.or(case_error),
                    steps: step_results,
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
    if let Some(error) = &result.error {
        return (false, None, Some(error.clone()));
    }
    let Some(output) = &result.output else {
        return (
            false,
            None,
            Some("adapter returned neither output nor error".into()),
        );
    };
    match match_output(output, expected, predicate) {
        Ok(MatchKind::Exact) => (true, Some("exact".into()), None),
        Ok(MatchKind::Predicate) => (true, Some("predicate".into()), None),
        Err(error) => (false, None, Some(error)),
    }
}

/// Small JSON-lines-free standard batch envelope for language adapters.
pub fn batch_input(
    manifest: &Manifest,
    package_prefix: &Path,
    requests: &[ExecutionRequest],
) -> Value {
    json!({
        "contract": manifest.name,
        "package": package_prefix.to_string_lossy().replace('\\', "/"),
        "validator": package_prefix.join(&manifest.files.validator).to_string_lossy().replace('\\', "/"),
        "requests": requests,
    })
}

pub fn tree_batch_input(batches: &[ContractBatch]) -> Value {
    json!({
        "contracts": batches.iter().map(|batch| {
            batch_input(&batch.manifest.manifest, &batch.package_prefix, &batch.requests)
        }).collect::<Vec<_>>()
    })
}
