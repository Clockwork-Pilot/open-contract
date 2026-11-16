use open_contract_language_rust::run_dataset;
use serde_json::Value;
use std::io::{self, Read};

mod validator;
mod implementation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let request: Value = serde_json::from_str(&text)?;
    let report = run_dataset(&request, validator::validate_case).map_err(io::Error::other)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
