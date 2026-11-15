#[path = "../tests/support/validator.rs"]
mod support;

fn main() -> std::process::ExitCode {
    support::entrypoint()
}
