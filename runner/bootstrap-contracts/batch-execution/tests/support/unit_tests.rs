use super::*;

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
        let Ok((requests, checks)) = flatten_dataset(&inner) else {
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
    };
    let empty = std::collections::HashMap::new();
    let (coverage, error) =
        coverage_for_key(&empty, &key, Some("0123456789abcdef0123456789abcdef"));
    assert!(coverage.is_none());
    assert!(error.unwrap().contains("coverage fingerprint unavailable"));

    let files = vec![CoverageFile {
        path: "validator.py".into(),
        lines: vec![],
    }];
    let coverage = coverage_for_files(files).unwrap();
    assert_eq!(coverage.fingerprint, coverage_hash(&[]));
    assert!(coverage.expected_fingerprint.is_none());
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
        },
        output: None,
        error: Some("invalid input".into()),
    };
    assert!(
        evaluate_result(
            &result,
            &serde_json::json!({}),
            Some("contains(.error, 'invalid input')")
        )
        .0
    );
    assert!(
        !evaluate_result(
            &result,
            &serde_json::json!({}),
            Some("contains(.error, 'different')")
        )
        .0
    );
    let empty = AdapterResult {
        error: None,
        ..result
    };
    assert!(evaluate_result(&empty, &serde_json::json!({}), None)
        .2
        .unwrap()
        .contains("neither output nor error"));
}
