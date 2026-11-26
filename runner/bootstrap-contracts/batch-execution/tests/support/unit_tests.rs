use super::*;

#[test]
fn adapter_coverage_schema_rejects_covered_lines_outside_executable_set() {
    let valid = json!({
        "coverage_lines": [{"path":"validator.py", "executable_lines":[1, 2], "covered_lines":[1]}]
    });
    assert!(validate_adapter_coverage_payload(&valid, "schema-fixture").is_ok());

    let invalid = json!({
        "coverage_lines": [{"path":"validator.py", "executable_lines":[1], "covered_lines":[2]}]
    });
    let error = validate_adapter_coverage_payload(&invalid, "schema-fixture").unwrap_err();
    assert!(error.contains("schema-fixture"));
    assert!(error.contains("every covered line must be executable"));
}

#[test]
fn adapter_coverage_schema_rejects_duplicate_and_unsorted_lines() {
    let unsorted = json!({"coverage_lines": [{"path":"validator.py", "executable_lines":[2, 1], "covered_lines":[]}]});
    assert!(
        validate_adapter_coverage_payload(&unsorted, "schema-fixture")
            .unwrap_err()
            .contains("sorted and unique")
    );
    let duplicate = json!({"coverage_lines": [{"path":"validator.py", "executable_lines":[1, 1], "covered_lines":[]}]});
    assert!(validate_adapter_coverage_payload(&duplicate, "schema-fixture").is_err());
}

#[test]
fn adapter_coverage_metric_counts_must_match_their_files() {
    let valid = json!({
        "coverage_lines": [{"path":"validator.py", "executable_lines":[1, 2], "covered_lines":[1]}],
        "coverage_metrics": {
            "statement": {
                "covered": 1,
                "total": 2,
                "files": [{"path":"validator.py", "covered":1, "total":2}]
            }
        }
    });
    assert!(validate_adapter_coverage_payload(&valid, "metric-fixture").is_ok());

    let mismatched = json!({
        "coverage_lines": [],
        "coverage_metrics": {
            "function": {
                "covered": 1,
                "total": 3,
                "files": [{"path":"validator.py", "covered":1, "total":2}]
            }
        }
    });
    assert!(validate_adapter_coverage_payload(&mismatched, "metric-fixture")
        .unwrap_err()
        .contains("file counts do not add up"));
}

#[test]
fn unavailable_metrics_remain_unavailable_under_the_shared_threshold() {
    let reported = std::collections::BTreeMap::from([(
        "function".to_owned(),
        CoverageMetricCounts {
            covered: 1,
            total: 2,
            files: vec![CoverageMetricFile {
                path: "validator.py".into(),
                covered: 1,
                total: 2,
            }],
        },
    )]);
    let lines = vec![CoverageLineFile {
        path: "validator.py".into(),
        executable_lines: vec![1],
        covered_lines: vec![1],
    }];
    let outcomes = coverage_metric_outcomes(&reported, &lines, 100.0);
    assert_eq!(outcomes["line"].threshold_met, Some(true));
    assert_eq!(outcomes["function"].threshold_met, Some(false));
    assert_eq!(outcomes["instantiation"].status, "unavailable");
    assert_eq!(outcomes["instantiation"].threshold_met, None);
    assert_eq!(outcomes["statement"].status, "unavailable");
    assert_eq!(outcomes["statement"].threshold_met, None);
}

#[test]
fn every_metric_threshold_accepts_exact_and_rejects_below_threshold_values() {
    for name in coverage_metric_names() {
        let is_line = *name == "line";
        let exact_reported = if is_line {
            std::collections::BTreeMap::new()
        } else {
            std::collections::BTreeMap::from([(
                (*name).to_owned(),
                CoverageMetricCounts {
                    covered: 1,
                    total: 1,
                    files: vec![CoverageMetricFile {
                        path: "validator.py".into(),
                        covered: 1,
                        total: 1,
                    }],
                },
            )])
        };
        let exact_lines = vec![CoverageLineFile {
            path: "validator.py".into(),
            executable_lines: vec![1],
            covered_lines: vec![1],
        }];
        let exact = coverage_metric_outcomes(&exact_reported, &exact_lines, 100.0);
        assert_eq!(exact[*name].percent, Some(100.0), "{name} exact threshold");
        assert_eq!(exact[*name].threshold_met, Some(true), "{name} exact threshold");

        let below_reported = if is_line {
            std::collections::BTreeMap::new()
        } else {
            std::collections::BTreeMap::from([(
                (*name).to_owned(),
                CoverageMetricCounts {
                    covered: 1,
                    total: 2,
                    files: vec![CoverageMetricFile {
                        path: "validator.py".into(),
                        covered: 1,
                        total: 2,
                    }],
                },
            )])
        };
        let below_lines = vec![CoverageLineFile {
            path: "validator.py".into(),
            executable_lines: vec![1, 2],
            covered_lines: vec![1],
        }];
        let below = coverage_metric_outcomes(
            &below_reported,
            &below_lines,
            100.0,
        );
        assert_eq!(below[*name].percent, Some(50.0), "{name} below threshold");
        assert_eq!(below[*name].threshold_met, Some(false), "{name} below threshold");
    }
}

#[test]
fn adapter_coverage_rejects_sources_outside_manifest_whitelist() {
    let valid = AdapterBatch {
        coverage: Some(json!({"sources": ["src/public.rs"]})),
        coverage_lines: vec![CoverageLineFile {
            path: "src/public.rs".into(),
            executable_lines: vec![1],
            covered_lines: vec![1],
        }],
        ..AdapterBatch::default()
    };
    assert!(validate_declared_coverage_sources(
        &valid,
        &["src/public.rs".into()],
        "whitelist-fixture"
    )
    .is_ok());

    let leaked = AdapterBatch {
        coverage: Some(json!({"sources": ["src/public.rs", "/rustc/library/std/src/lib.rs"]})),
        ..valid
    };
    let error = validate_declared_coverage_sources(
        &leaked,
        &["src/public.rs".into()],
        "whitelist-fixture",
    )
    .unwrap_err();
    assert!(error.contains("outside coverage_sources"));

    let leaked_lines = AdapterBatch {
        coverage_lines: vec![CoverageLineFile {
            path: "src/helper.rs".into(),
            executable_lines: vec![1],
            covered_lines: vec![1],
        }],
        ..AdapterBatch::default()
    };
    let error = validate_declared_coverage_sources(
        &leaked_lines,
        &["src/public.rs".into()],
        "whitelist-fixture",
    )
    .unwrap_err();
    assert!(error.contains("coverage_sources"));
}

#[test]
fn dataset_contract_cases_are_reused_as_unit_inputs() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("dataset.json");
    let outer =
        bootstrap_dataset_harness::load_dataset(&path).expect("bootstrap dataset should parse");
    let mut checked = 0;
    for entry in outer.cases {
        if entry.input.get("extension").is_some() {
            continue;
        }
        let Some(value) = entry.input.get("dataset") else {
            continue;
        };
        let parsed = serde_json::from_value::<Dataset>(value.clone());
        let Ok(inner) = parsed else { continue };
        let Ok((requests, checks)) = flatten_dataset_with_fingerprints(&inner, true) else {
            continue;
        };
        assert!(
            !requests.is_empty(),
            "{} should request execution",
            entry.id
        );
        let expected_requests = inner
            .cases
            .iter()
            .map(|case| match case {
                CaseSpec::Call(_) => 1,
                CaseSpec::Sequence(sequence) => sequence.steps.len(),
            })
            .sum::<usize>();
        assert_eq!(
            requests.len(),
            expected_requests,
            "{} request count",
            entry.id
        );
        assert_eq!(checks.len(), inner.cases.len(), "{} case count", entry.id);
        checked += 1;
    }
    assert!(checked >= 20, "expected to reuse the public dataset cases");
}

#[test]
fn adapter_result_index_rejects_duplicate_keys() {
    let key = ExecutionKey {
        case_id: "c".into(),
        step_id: None,
    dataset_id: None,
    };
    let result = AdapterResult {
        key: key.clone(),
        output: Some(Value::Null),
        error: None,
    };
    let error = index_results(vec![result.clone(), result]).unwrap_err();
    assert!(error.contains("duplicate result keys"));
}

#[test]
fn coverage_scope_validation_rejects_missing_and_invalid_fingerprints() {
    assert!(validate_fingerprint(&None, "case")
        .unwrap_err()
        .contains("is required"));
    assert!(validate_fingerprint(&Some("XYZ".into()), "case").is_err());
    assert!(
        validate_fingerprint(&Some("00000000000000000000000000000000".into()), "case").is_err()
    );
    assert!(validate_fingerprint(&Some("0123456789abcdef0123456789abcdef".into()), "case").is_ok());

    let key = ExecutionKey {
        case_id: "c".into(),
        step_id: None,
    dataset_id: None,
    };
    let empty = std::collections::HashMap::new();
    let (coverage, error) =
        coverage_for_key(&empty, &key, Some("0123456789abcdef0123456789abcdef"), true);
    assert!(coverage.is_none());
    assert!(error.unwrap().contains("coverage fingerprint unavailable"));

    let files = vec![CoverageFile {
        path: "validator.py".into(),
        lines: vec![],
    }];
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("validator.py"), "").unwrap();
    let coverage = coverage_for_files(files, &std::collections::HashMap::new()).unwrap();
    assert_eq!(
        coverage.fingerprint,
        coverage_hash_from_source_lines(&[], &std::collections::HashMap::new())
    );
    assert!(coverage.expected_fingerprint.is_none());
}

#[test]
fn coverage_fingerprint_hashes_line_text_without_line_numbers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("validator.rs");
    std::fs::write(&path, "first\nsecond\n").unwrap();
    let before = coverage_hash(
        &[CoverageFile {
            path: "validator.rs".into(),
            lines: vec![1, 2],
        }],
        root.path(),
    )
    .unwrap()
    .0;

    std::fs::write(&path, "// inserted line\nfirst\nsecond\n").unwrap();
    let after = coverage_hash(
        &[CoverageFile {
            path: "validator.rs".into(),
            lines: vec![2, 3],
        }],
        root.path(),
    )
    .unwrap()
    .0;
    assert_eq!(before, after);

    std::fs::write(&path, "// inserted line\nfirst changed\nsecond\n").unwrap();
    let changed = coverage_hash(
        &[CoverageFile {
            path: "validator.rs".into(),
            lines: vec![2, 3],
        }],
        root.path(),
    )
    .unwrap()
    .0;
    assert_ne!(after, changed);
}

#[test]
fn coverage_files_are_canonical_and_duplicate_lines_merge() {
    let merged = merge_coverage_files(vec![
        CoverageFile {
            path: "validator.py".into(),
            lines: vec![2, 1],
        },
        CoverageFile {
            path: "validator.py".into(),
            lines: vec![2, 3],
        },
    ]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].path, "validator.py");
    assert_eq!(merged[0].lines, vec![1, 2, 3]);
    assert!(canonical_coverage_files(vec![CoverageFile {
        path: "validator.py".into(),
        lines: vec![0]
    }])
    .is_err());
}

#[test]
fn verification_matches_error_outputs_and_rejects_missing_outputs() {
    let result = AdapterResult {
        key: ExecutionKey {
            case_id: "c".into(),
            step_id: None,
            dataset_id: None,
        },
        output: None,
        error: Some("invalid input".into()),
    };
    assert!(
        evaluate_result(
            &result,
            Some(&serde_json::json!({})),
            Some("contains(.error, 'invalid input')")
        )
        .0
    );
    assert!(
        !evaluate_result(
            &result,
            Some(&serde_json::json!({})),
            Some("contains(.error, 'different')")
        )
        .0
    );
    let empty = AdapterResult {
        error: None,
        ..result
    };
    assert!(evaluate_result(&empty, Some(&serde_json::json!({})), None)
        .2
        .unwrap()
        .contains("neither output nor error"));
}
