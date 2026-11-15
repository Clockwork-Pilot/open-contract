use std::path::PathBuf;
use std::process::ExitCode;

use bootstrap_dataset_harness::run_dataset;

#[path = "../tests/support/validator.rs"]
mod validator;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("batch-execution dataset: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases = run_dataset(package.join("dataset.json"), |_, case| {
        validator::validate_dataset_entry(case)
    })?;
    println!("Batch execution dataset: {cases} cases passed");
    Ok(())
}
