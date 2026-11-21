use super::*;
use serde_json::json;

#[test]
fn manifest_paths_cannot_escape_through_symlinks() {
    let read_escape = json!({
        "yaml": "version: '1'\nname: sample\ndatasets: [escape], coverage_sources: [validator.py]\nfilesystem: {read: [escape, validator.py], write: []}\n"
    });
    assert!(!run_manifest_case(&read_escape, Some("read-path-symlink-escape")).unwrap());

    let write_escape = json!({
        "yaml": "version: '1'\nname: sample\ndatasets: [dataset.json], coverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: [escape/outside.txt]}\n"
    });
    assert!(!run_manifest_case(&write_escape, Some("write-path-symlink-escape")).unwrap());
}

#[test]
fn manifest_path_read_failures_are_unit_fixtures() {
    let missing = tempfile::tempdir().unwrap();
    assert!(ManifestPlan::load(&missing.path().join("missing.yaml")).is_err());

    let directory = tempfile::tempdir().unwrap();
    assert!(ManifestPlan::load(directory.path()).is_err());

    assert!(ManifestPlan::load("/").is_err());
}
