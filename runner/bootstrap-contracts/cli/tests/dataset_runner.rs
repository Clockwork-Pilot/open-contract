pub use runner_cli::*;

#[path = "support/validator.rs"]
mod validator;

#[test]
fn dataset_entries_exercise_public_api() {
    let cases = validator::execute_dataset().expect("CLI dataset cases should pass");
    assert!(cases > 0, "CLI dataset must contain cases");
    html_report_write_errors_remain_actionable();
}

fn html_report_write_errors_remain_actionable() {
    use std::fs;

    for (directory, blocker, expected_error) in [
        ("html-blocked", "file", "cannot create HTML coverage directory"),
        ("html-page", "page", "cannot write HTML coverage report"),
        ("html-index", "index", "cannot write HTML coverage index"),
    ] {
        let root = tempfile::tempdir().expect("temporary CLI fixture directory");
        write_html_fixture(root.path(), include_str!("../fixtures/adapter.py"));
        let html = root.path().join(directory);
        match blocker {
            "file" => fs::write(&html, "not a directory").expect("blocker file"),
            "page" => fs::create_dir_all(html.join("contract-000.html")).expect("page blocker"),
            "index" => fs::create_dir_all(html.join("index.html")).expect("index blocker"),
            _ => unreachable!(),
        }

        let result = run_html_fixture(root.path(), directory);
        assert_eq!(result.exit_code, 2, "{}", result.stderr);
        assert!(result.stderr.contains(expected_error), "{}", result.stderr);
    }

    let root = tempfile::tempdir().expect("temporary CLI fixture directory");
    let no_coverage = "import json, sys\nrequest = json.load(sys.stdin)\nprint(json.dumps({\"results\": [{\"key\": item[\"key\"], \"output\": item[\"input\"]} for item in request[\"requests\"]], \"coverage\": {\"sources\": []}, \"coverage_lines\": []}))\n";
    write_html_fixture(root.path(), no_coverage);
    let result = run_html_fixture(root.path(), "html-empty");
    assert!(root.path().join("html-empty/index.html").is_file(), "{}", result.stderr);
    let page = std::fs::read_to_string(root.path().join("html-empty/contract-000.html"))
        .expect("HTML contract page");
    assert!(page.contains("no source locations"));

}

fn write_html_fixture(root: &std::path::Path, adapter: &str) {
    use std::fs;
    for (path, contents) in [
        ("open-contract.yaml", include_str!("../fixtures/contract.yaml")),
        ("dataset.json", r#"{"version":1,"cases":[{"id":"echo","input":{"value":7},"expected":{"value":7}}]}"#),
        ("adapter.py", adapter),
        ("validator.py", include_str!("../fixtures/validator.py")),
        ("tests/check.py", include_str!("../fixtures/tests/check.py")),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).expect("fixture parent directory");
        fs::write(path, contents).expect("fixture file");
    }
}

fn run_html_fixture(root: &std::path::Path, output: &str) -> runner_cli::CliOutput {
    let previous = std::env::current_dir().expect("current directory");
    std::env::set_current_dir(root).expect("enter fixture directory");
    let result = runner_cli::execute(&[
        "run-manifest".into(),
        "open-contract.yaml".into(),
        "--isolation".into(),
        "none".into(),
        "--disable-fingerprints".into(),
        "--coverage-report".into(),
        "coverage.json".into(),
        "--coverage-html".into(),
        output.into(),
    ]);
    std::env::set_current_dir(previous).expect("restore current directory");
    result
}
