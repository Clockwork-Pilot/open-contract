use runner_dataset_generation::generate_dataset;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dataset generation: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let schema_path = args
        .next()
        .ok_or_else(|| "usage: generate-dataset <schema.yaml|schema.json>".to_string())?;
    if args.next().is_some() {
        return Err("usage: generate-dataset <schema.yaml|schema.json>".into());
    }
    let path = std::path::PathBuf::from(schema_path);
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read schema {}: {error}", path.display()))?;
    let schema = serde_yaml::from_str(&source)
        .map_err(|error| format!("invalid schema {}: {error}", path.display()))?;
    let dataset = generate_dataset(&schema)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&dataset)
            .map_err(|error| format!("cannot encode generated dataset: {error}"))?
    );
    Ok(())
}
