    use super::*;

    #[test]
    fn dataset_contract_cases_are_reused_as_unit_inputs() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("dataset.json");
        let outer = load_dataset(&path).expect("bootstrap dataset should parse");
        let cases = outer.cases.expect("dataset wrapper has cases");
        let mut checked = 0;
        for case in cases {
            if case.input.get("extension").is_some() {
                continue;
            }
            let Some(value) = case.input.get("dataset") else {
                continue;
            };
            let inner: Dataset = serde_json::from_value(value.clone())
                .expect("embedded public dataset input should deserialize");
            let result = flatten_dataset(&inner);
            if case
                .expected
                .as_ref()
                .is_some_and(|expected| expected.get("error_contains").is_some())
            {
                assert!(result.is_err(), "{} should be rejected", case.id);
            } else {
                let (requests, checks) = result.expect("public dataset input should flatten");
                assert!(!requests.is_empty(), "{} should request execution", case.id);
                assert_eq!(
                    requests.len(),
                    checks.len()
                        + requests
                            .iter()
                            .filter(|r| r.key.step_id.is_some())
                            .count()
                            .saturating_sub(
                                checks
                                    .iter()
                                    .filter(|check| matches!(check, Check::Sequence { .. }))
                                    .count()
                            ),
                    "{} request/check structure",
                    case.id
                );
            }
            checked += 1;
        }
        assert!(checked >= 20, "expected to reuse the public dataset cases");
    }

    #[test]
    fn adapter_result_index_rejects_duplicate_keys() {
        let key = ExecutionKey {
            target_id: "t".into(),
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
        assert!(
            validate_fingerprint(&Some("0123456789abcdef0123456789abcdef".into()), "case").is_ok()
        );

        let key = ExecutionKey {
            target_id: "t".into(),
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
        let (coverage, error) = coverage_for_files(files, Some("0123456789abcdef0123456789abcdef"));
        assert!(coverage.is_none());
        assert!(error.unwrap().contains("zero covered lines"));
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
    fn verification_handles_expected_errors_and_missing_outputs() {
        let result = AdapterResult {
            key: ExecutionKey {
                target_id: "t".into(),
                case_id: "c".into(),
                step_id: None,
            },
            output: None,
            error: Some("invalid input".into()),
        };
        assert!(evaluate_result(&result, None, None, Some("invalid input")).0);
        assert!(!evaluate_result(&result, None, None, Some("different")).0);
        let empty = AdapterResult {
            error: None,
            ..result
        };
        assert!(evaluate_result(&empty, None, None, None)
            .2
            .unwrap()
            .contains("neither output nor error"));
    }
