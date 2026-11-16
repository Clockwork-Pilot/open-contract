use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let response = runner_cli::execute(&arguments);
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    if stdout.write_all(response.stdout.as_bytes()).is_err()
        || stderr.write_all(response.stderr.as_bytes()).is_err()
    {
        return ExitCode::from(2);
    }
    ExitCode::from(response.exit_code)
}
