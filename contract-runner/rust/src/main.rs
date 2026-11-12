use open_contract_core::{run_contract_tree, Backend, CommandBatchAdapter, RuntimeBatchAdapter};
use std::path::PathBuf;

fn main() {
    match run() {
        Ok(()) => {}
        Err((code, message)) => {
            eprintln!("open-contract-core: {message}");
            std::process::exit(code);
        }
    }
}

fn run() -> Result<(), (i32, String)> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("run-manifest") {
        return Err((2, "usage: open-contract-core run-manifest <contract.yaml> --isolation <none|bwrap|landlock> [--runtime-profile <python|javascript>] [--adapter <command>]".into()));
    }
    let manifest = PathBuf::from(args.next().ok_or((2, "missing manifest path".into()))?);
    let mut backend = None;
    let mut runtime_profile = None;
    let mut adapter = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolation" => {
                backend = Some(parse_backend(
                    &args.next().ok_or((2, "missing isolation backend".into()))?,
                )?)
            }
            "--runtime-profile" => {
                runtime_profile = Some(args.next().ok_or((2, "missing runtime profile".into()))?)
            }
            "--adapter" => {
                adapter.push(args.next().ok_or((2, "missing adapter command".into()))?);
                adapter.extend(args);
                break;
            }
            other => return Err((2, format!("unknown argument: {other}"))),
        }
    }
    let backend = backend.ok_or((2, "--isolation is required".into()))?;
    let report = if adapter.is_empty() {
        run_contract_tree(manifest, backend, &RuntimeBatchAdapter)
    } else {
        run_contract_tree(
            manifest,
            backend,
            &CommandBatchAdapter {
                command: adapter,
                runtime_profile,
            },
        )
    }
    .map_err(|e| (2, e))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| (2, e.to_string()))?
    );
    if report.passed() != report.total() {
        return Err((
            1,
            format!(
                "{} of {} contract cases passed",
                report.passed(),
                report.total()
            ),
        ));
    }
    if !report.coverage_complete() {
        let missing = report
            .coverage
            .iter()
            .filter(|record| !record.missing_sources.is_empty())
            .map(|record| format!("{}: {}", record.contract, record.missing_sources.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        return Err((
            1,
            format!("coverage did not exercise every declared contract source: {missing}"),
        ));
    }
    Ok(())
}

fn parse_backend(value: &str) -> Result<Backend, (i32, String)> {
    match value {
        "none" => Ok(Backend::None),
        "bwrap" => Ok(Backend::Bubblewrap),
        "landlock" => Ok(Backend::Landlock),
        _ => Err((
            2,
            format!("invalid isolation backend {value:?}; expected none, bwrap, or landlock"),
        )),
    }
}
