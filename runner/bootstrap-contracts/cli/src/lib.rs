use runner_batch::{run_contract_tree_with_runtime, show_isolation_plan, BatchReport, ManifestBatchAdapter};
use runner_isolation::Backend;
use runner_errors::ContractError;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Eq, PartialEq)]
pub struct CliOutput {
    pub exit_code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Execute the Open Contract Rust CLI with argument strings excluding argv[0].
/// The binary and bootstrap dataset both call this public entry point.
pub fn execute(arguments: &[String]) -> CliOutput {
    let mut stdout = String::new();
    match run(arguments.iter().cloned(), &mut stdout) {
        Ok(()) => CliOutput {
            exit_code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            exit_code: error.exit_code(),
            stdout,
            stderr: format!("open-contract-core: {error}\n"),
        },
    }
}

fn run(mut args: impl Iterator<Item = String>, stdout: &mut String) -> Result<(), ContractError> {
    let subcommand = args.next();
    if subcommand.as_deref() == Some("show-isolation-plan") {
        stdout.push_str(&show_plan(args)?);
        return Ok(());
    }
    if subcommand.as_deref() != Some("run-manifest") {
        return Err((2, "usage: open-contract-core <run-manifest|show-isolation-plan> <contract.yaml> --isolation <none|bwrap|landlock> [--isolation-runtime <path>] [--coverage-report <path>]".into()).into());
    }
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    let mut backend = None;
    let mut isolation_runtime = None;
    let mut coverage_report = PathBuf::from(".open-contract/coverage/latest.json");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_backend(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?)
            }
            "--coverage-report" => {
                coverage_report = PathBuf::from(
                    args.next().ok_or((2, "missing coverage report path".into()))?,
                );
            }
            "--isolation-runtime" => {
                isolation_runtime = Some(PathBuf::from(
                    args.next().ok_or((2, "missing isolation runtime path".into()))?,
                ));
            }
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    let backend = backend.ok_or((2, "--isolation is required".into()))?;
    if backend == Backend::None && isolation_runtime.is_some() {
        return Err((2, "--isolation-runtime cannot be used with --isolation none".into()).into());
    }
    let report = run_contract_tree_with_runtime(manifest, backend, isolation_runtime, &ManifestBatchAdapter)
        .map_err(|error| ContractError::from((2, error)).context("contract execution"))?;
    write_adapter_coverage_reports(&coverage_report, &report)?;
    stdout.push_str(&concise_summary(&report));
    stdout.push_str(&format!("Coverage report: {}\n", coverage_report.display()));
    if report.passed() != report.total() {
        return Err((1, format!("{}/{} dataset cases passed across {} contract(s)", report.passed(), report.total(), report.coverage.len())).into());
    }
    if !report.coverage_complete() {
        return Err((1, "coverage requirements not met; see adapter coverage report".into()).into());
    }
    Ok(())
}

fn show_plan(mut args: impl Iterator<Item = String>) -> Result<String, ContractError> {
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    let mut backend = None;
    let mut isolation_runtime = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_backend(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?);
            }
            "--isolation-runtime" => {
                isolation_runtime = Some(PathBuf::from(
                    args.next()
                        .ok_or((2, "missing isolation runtime path".into()))?,
                ));
            }
            other => return Err((2, format!("unknown argument: {other}")).into()),
        }
    }
    let backend = backend.ok_or((2, "--isolation is required".into()))?;
    if backend == Backend::None && isolation_runtime.is_some() {
        return Err((
            2,
            "--isolation-runtime cannot be used with --isolation none".into(),
        )
            .into());
    }
    show_isolation_plan(manifest, backend, isolation_runtime)
        .map_err(|error| ContractError::from((2, error)).context("isolation plan"))
}

/// Persist adapter reports without interpreting or normalizing their contents.
fn write_adapter_coverage_reports(
    path: &std::path::Path,
    report: &BatchReport,
) -> Result<(), ContractError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            (
                2,
                format!(
                    "cannot create coverage report directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let reports = report
        .coverage
        .iter()
        .map(|record| {
            json!({
                "contract": record.contract,
                "report": record.native_report,
            })
        })
        .collect::<Vec<Value>>();
    let contents = json!({ "contracts": reports }).to_string();
    fs::write(path, contents).map_err(|error| {
        ContractError::from((
            2,
            format!("cannot write coverage report {}: {error}", path.display()),
        ))
    })
}

fn concise_summary(report: &BatchReport) -> String {
    let mut output = format!(
        "Dataset cases: {}/{} passed across {} contract(s)\n",
        report.passed(),
        report.total(),
        report.coverage.len()
    );
    for result in report.results.iter().filter(|result| !result.passed) {
        output.push_str(&format!(
            "  FAIL {}/{}/{}: {}\n",
            result.contract,
            result.target_id,
            result.case_id,
            result
                .error
                .as_deref()
                .unwrap_or("case verification failed")
        ));
        output.push_str(&format!(
            "    actual: {}\n",
            result.output.as_ref().unwrap_or(&Value::Null)
        ));
    }
    output
}

fn parse_backend(value: &str) -> Result<Backend, ContractError> {
    match value {
        "none" => Ok(Backend::None),
        "bwrap" => Ok(Backend::Bubblewrap),
        "landlock" => Ok(Backend::Landlock),
        _ => Err((2, format!("invalid isolation backend {value:?}; expected none, bwrap, or landlock")).into()),
    }
}

#[cfg(test)]
#[path = "../tests/unit_dataset_runner.rs"]
mod dataset_runner_test;
