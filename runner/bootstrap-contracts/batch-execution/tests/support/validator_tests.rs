use super::*;
use runner_batch::{
    AdapterBatch, AdapterResult, BatchAdapter, ContractBatch, CoverageFile, CoverageScope,
    ExecutionKey, ManifestBatchAdapter,
};
use runner_isolation::{Backend, StagedTree};

pub(super) fn configure_supplemental_fixture(
    input: &Value,
    package: &Path,
    manifest: &mut Value,
) -> Result<(), String> {
    if input["supplemental_tests"].as_bool() != Some(true) {
        return Ok(());
    }
    fs::create_dir_all(package.join("tests"))
        .map_err(|error| format!("create supplemental test directory: {error}"))?;
    write(
        package.join("tests/supplemental.py"),
        "def run(): return True\n",
    )?;
    manifest["files"]["supplemental_tests"] = json!(["tests/supplemental.py"]);
    manifest["filesystem"]["read"]
        .as_array_mut()
        .ok_or_else(|| "fixture read list must be an array".to_string())?
        .push(json!("tests/supplemental.py"));
    Ok(())
}

pub(super) fn check_supplemental_status(
    report: &runner_batch::BatchReport,
    expected: &Value,
) -> Result<(), String> {
    let Some(status) = expected["supplemental_status"].as_str() else {
        return Ok(());
    };
    let observed = report
        .coverage
        .first()
        .and_then(|record| record.additional_test_coverage["status"].as_str())
        .ok_or_else(|| "supplemental coverage status was missing".to_string())?;
    if observed != status {
        return Err(format!(
            "expected supplemental status {status:?}, got {observed:?}"
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct HarnessScenario {
    id: &'static str,
    input: Value,
    setup: Value,
    expected: Value,
    adapter_behavior: Option<&'static str>,
}

#[test]
fn harness_edge_cases_are_unit_tests_not_public_dataset_cases() {
    for scenario in harness_scenarios() {
        validate_harness_case(&scenario)
            .unwrap_or_else(|error| panic!("unit case {} failed: {error}", scenario.id));
    }
}

fn harness_scenarios() -> Vec<HarnessScenario> {
    vec![
        HarnessScenario {
            id: "dataset-read-error",
            input: json!({}),
            setup: json!({
                "operation": "dataset-read-error"
            }),
            expected: json!({
                "error_contains": "No such file or directory"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "expected-validator-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected_error": "fixture error",
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": true
            }),
            adapter_behavior: Some("case-error"),
        },
        HarnessScenario {
            id: "unexpected-validator-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "fixture error"
            }),
            adapter_behavior: Some("case-error"),
        },
        HarnessScenario {
            id: "wrong-expected-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected_error": "other error",
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "expected error"
            }),
            adapter_behavior: Some("case-error"),
        },
        HarnessScenario {
            id: "missing-output-and-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "neither output nor error"
            }),
            adapter_behavior: Some("no-output"),
        },
        HarnessScenario {
            id: "missing-case-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "fingerprint unavailable"
            }),
            adapter_behavior: Some("no-coverage"),
        },
        HarnessScenario {
            id: "unasserted-step-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "sequence",
                            "input": {},
                            "steps": [
                                {
                                    "id": "first",
                                    "input": {
                                        "value": 1
                                    },
                                    "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                                }
                            ],
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "fixture error"
            }),
            adapter_behavior: Some("case-error"),
        },
        HarnessScenario {
            id: "sequence-missing-step-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "sequence",
                            "input": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8",
                            "steps": [
                                {
                                    "id": "first",
                                    "input": {
                                        "value": 1
                                    },
                                    "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                                }
                            ]
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "fingerprint unavailable"
            }),
            adapter_behavior: Some("no-coverage"),
        },
        HarnessScenario {
            id: "adapter-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "fixture adapter failed"
            }),
            adapter_behavior: Some("adapter-error"),
        },
        HarnessScenario {
            id: "wrong-batch-count",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "returned 0 contract batches"
            }),
            adapter_behavior: Some("no-batches"),
        },
        HarnessScenario {
            id: "no-results",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "omitted result"
            }),
            adapter_behavior: Some("no-results"),
        },
        HarnessScenario {
            id: "duplicate-results",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "duplicate result keys"
            }),
            adapter_behavior: Some("duplicate-results"),
        },
        HarnessScenario {
            id: "extra-results",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "not requested"
            }),
            adapter_behavior: Some("extra-results"),
        },
        HarnessScenario {
            id: "unrequested-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "unrequested execution key"
            }),
            adapter_behavior: Some("unrequested-coverage"),
        },
        HarnessScenario {
            id: "undeclared-coverage-file",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "undeclared contract source"
            }),
            adapter_behavior: Some("undeclared-coverage"),
        },
        HarnessScenario {
            id: "invalid-coverage-path",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "must stay within the package"
            }),
            adapter_behavior: Some("invalid-coverage-path"),
        },
        HarnessScenario {
            id: "zero-coverage-line",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "line numbers must be positive"
            }),
            adapter_behavior: Some("zero-coverage-line"),
        },
        HarnessScenario {
            id: "pinned-fingerprint-rejects-empty-case-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "zero covered lines"
            }),
            adapter_behavior: Some("empty-coverage"),
        },
        HarnessScenario {
            id: "pinned-fingerprint-rejects-empty-sequence-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "sequence",
                            "input": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8",
                            "steps": [
                                {
                                    "id": "step",
                                    "input": {
                                        "value": 1
                                    },
                                    "expected": {
                                        "value": 1
                                    },
                                    "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                                }
                            ]
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "zero covered lines"
            }),
            adapter_behavior: Some("empty-coverage"),
        },
        HarnessScenario {
            id: "duplicate-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "duplicate coverage scope"
            }),
            adapter_behavior: Some("duplicate-coverage"),
        },
        HarnessScenario {
            id: "nested-contract",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "nested_contract": true
            }),
            expected: json!({
                "passed": true,
                "total": 2
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "duplicate-nested-name",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "duplicate_nested_name": true
            }),
            expected: json!({
                "error_contains": "duplicate contract name"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "nested-cycle",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "nested_cycle": true
            }),
            expected: json!({
                "error_contains": "cycle"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-missing-execution",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest"
            }),
            expected: json!({
                "error_contains": "missing execution.command"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-command-failure",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "sh",
                    "-c",
                    "echo failed >&2; exit 7"
                ]
            }),
            expected: json!({
                "error_contains": "execution.command failed"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-invalid-json",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "sh",
                    "-c",
                    "printf not-json"
                ]
            }),
            expected: json!({
                "error_contains": "invalid batch JSON"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-duplicate-result-key",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "python3",
                    "-c",
                    "import json,sys; r=json.load(sys.stdin)['requests'][0]; print(json.dumps({'results':[{'key':r['key'],'output':{}},{'key':r['key'],'output':{}}]}))"
                ]
            }),
            expected: json!({
                "error_contains": "duplicate result keys"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-missing-batch",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "sh",
                    "-c",
                    "printf '{\"results\":[]}'"
                ]
            }),
            expected: json!({
                "error_contains": "returned no batch results"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-rejects-tree-envelope",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "sh",
                    "-c",
                    "printf '{\"contracts\":[]}'"
                ]
            }),
            expected: json!({
                "error_contains": "invalid batch JSON"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-command-start-error",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {},
                            "expected": {},
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "/no-such-open-contract-command"
                ]
            }),
            expected: json!({
                "error_contains": "No such file or directory"
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "manifest-success",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "8a484661678bcd54e5d968c1b7e473ec"
                        }
                    ]
                }
            }),
            setup: json!({
                "adapter": "manifest",
                "command": [
                    "python3",
                    "-c",
                    "import json,sys; x=json.load(sys.stdin); r=x['requests'][0]; p=x['validator']; print(json.dumps({'results':[{'key':r['key'],'output':r['input']}], 'coverage':{'sources':[p]}, 'coverage_scopes':[{'key':r['key'],'files':[{'path':p,'lines':[1,2]}]}], 'coverage_lines':[{'path':p,'executable_lines':[1,2],'covered_lines':[1,2]}]}))"
                ]
            }),
            expected: json!({
                "passed": true,
                "total": 1,
                "coverage_complete": true
            }),
            adapter_behavior: None,
        },
        HarnessScenario {
            id: "multi-file-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "642e4c8886d32153e0a01013d9244742"
                        }
                    ]
                }
            }),
            setup: json!({
                "support": true
            }),
            expected: json!({
                "passed": true
            }),
            adapter_behavior: Some("multi-file-coverage"),
        },
        HarnessScenario {
            id: "coverage-json-special-characters",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "2409964b5e80537c79cd2c94a6de7ce2"
                        }
                    ]
                }
            }),
            setup: json!({
                "support_path": "quote\"backspace\x08tab\tline\nfeed\x0Cform\rcontrol\u{0001}.py"
            }),
            expected: json!({
                "passed": true
            }),
            adapter_behavior: Some("multi-file-coverage"),
        },
        HarnessScenario {
            id: "sequence-required-fingerprint-without-coverage",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "sequence",
                            "input": {},
                            "steps": [
                                {
                                    "id": "step",
                                    "input": {
                                        "value": 1
                                    },
                                    "expected": {
                                        "value": 1
                                    },
                                    "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                                }
                            ],
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "passed": false,
                "result_error_contains": "coverage fingerprint unavailable"
            }),
            adapter_behavior: Some("no-coverage"),
        },
        HarnessScenario {
            id: "sequence-missing-result",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "sequence",
                            "input": {},
                            "steps": [
                                {
                                    "id": "step",
                                    "input": {
                                        "value": 1
                                    },
                                    "expected": {
                                        "value": 1
                                    },
                                    "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                                }
                            ],
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "omitted result"
            }),
            adapter_behavior: Some("omit-step-results"),
        },
        HarnessScenario {
            id: "nested-line-report-paths",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "nested_contract": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": true,
                "total": 2
            }),
            adapter_behavior: Some("line-coverage"),
        },
        HarnessScenario {
            id: "nested-command-without-native-source-list",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "nested_contract": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": false,
                "total": 2
            }),
            adapter_behavior: Some("coverage-without-sources"),
        },
        HarnessScenario {
            id: "supplemental-coverage-skipped-under-90",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": false,
                "supplemental_status": "skipped"
            }),
            adapter_behavior: Some("supplemental-under-threshold"),
        },
        HarnessScenario {
            id: "supplemental-coverage-skipped-without-line-data",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": false,
                "supplemental_status": "skipped"
            }),
            adapter_behavior: Some("supplemental-no-lines"),
        },
        HarnessScenario {
            id: "supplemental-test-passes",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": true,
                "supplemental_status": "passed"
            }),
            adapter_behavior: Some("supplemental-passed"),
        },
        HarnessScenario {
            id: "supplemental-test-fails",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "passed": true,
                "coverage_complete": false,
                "supplemental_status": "failed"
            }),
            adapter_behavior: Some("supplemental-failed"),
        },
        HarnessScenario {
            id: "supplemental-test-result-omitted",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "error_contains": "omitted supplemental test results"
            }),
            adapter_behavior: Some("supplemental-omitted"),
        },
        HarnessScenario {
            id: "supplemental-test-not-executed",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "error_contains": "did not execute manifest-declared supplemental tests"
            }),
            adapter_behavior: Some("supplemental-not-executed"),
        },
        HarnessScenario {
            id: "supplemental-data-without-declaration",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({}),
            expected: json!({
                "error_contains": "without manifest-declared tests"
            }),
            adapter_behavior: Some("unexpected-supplemental"),
        },
        HarnessScenario {
            id: "supplemental-tests-cannot-run-below-threshold",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "error_contains": "with only 50.0% dataset line coverage"
            }),
            adapter_behavior: Some("supplemental-under-threshold-ran"),
        },
        HarnessScenario {
            id: "supplemental-coverage-requires-test-execution",
            input: json!({
                "dataset": {
                    "version": 1,
                    "cases": [
                        {
                            "id": "one",
                            "input": {
                                "value": 1
                            },
                            "expected": {
                                "value": 1
                            },
                            "coverage_fingerprint": "60c6194cbdc58033999a1fb07d147da8"
                        }
                    ]
                }
            }),
            setup: json!({
                "supplemental_tests": true
            }),
            expected: json!({
                "error_contains": "without running its tests"
            }),
            adapter_behavior: Some("supplemental-under-threshold-coverage"),
        },
    ]
}

fn validate_harness_case(scenario: &HarnessScenario) -> Result<(), String> {
    let mut input = scenario.input.clone();
    let input_object = input
        .as_object_mut()
        .ok_or_else(|| "unit input must be an object".to_string())?;
    let setup = scenario
        .setup
        .as_object()
        .ok_or_else(|| "unit setup must be an object".to_string())?;
    input_object.extend(
        setup
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let case: DatasetCase = serde_json::from_value(json!({
        "id": scenario.id,
        "input": input,
        "expected": scenario.expected,
    }))
    .map_err(|error| format!("cannot construct unit input: {error}"))?;
    if case.input["adapter"].as_str() == Some("manifest") {
        return validate_dataset_entry_with_adapter(&case, &ManifestBatchAdapter);
    }
    let mode = scenario.adapter_behavior.unwrap_or("normal").to_owned();
    validate_dataset_entry_with_adapter(&case, &FixtureAdapter { mode })
}

struct FixtureAdapter {
    mode: String,
}

impl BatchAdapter for FixtureAdapter {
    fn execute_tree(
        &self,
        _tree: &StagedTree,
        _backend: Backend,
        batches: &[ContractBatch],
    ) -> Result<Vec<AdapterBatch>, String> {
        if self.mode == "adapter-error" {
            return Err("fixture adapter failed".into());
        }
        if self.mode == "no-batches" {
            return Ok(Vec::new());
        }
        Ok(batches
            .iter()
            .map(|batch| AdapterBatch {
                results: self.results(batch),
                coverage: if self.mode == "coverage-without-sources" {
                    Some(json!({"format": "fixture"}))
                } else {
                    Some(json!({
                        "format": "fixture",
                        "sources": std::iter::once(&batch.manifest.manifest.files.validator)
                            .chain(&batch.manifest.manifest.files.support)
                            .cloned()
                            .collect::<Vec<_>>(),
                    }))
                },
                coverage_scopes: self.coverage(batch),
                coverage_lines: self.line_coverage(batch),
                additional_test_coverage: self.supplemental_report(),
                additional_coverage_lines: self.additional_line_coverage(batch),
                ..AdapterBatch::default()
            })
            .collect())
    }
}

impl FixtureAdapter {
    fn line_coverage(&self, batch: &ContractBatch) -> Vec<runner_batch::CoverageLineFile> {
        if self.mode == "supplemental-no-lines" || self.mode == "no-coverage" {
            return Vec::new();
        }
        let (executable_lines, covered_lines) = if matches!(
            self.mode.as_str(),
            "supplemental-under-threshold"
                | "supplemental-under-threshold-ran"
                | "supplemental-under-threshold-coverage"
        ) {
            (vec![1, 2], vec![1])
        } else {
            (vec![1], vec![1])
        };
        vec![runner_batch::CoverageLineFile {
            path: batch.manifest.manifest.files.validator.clone(),
            executable_lines,
            covered_lines,
        }]
    }

    fn additional_line_coverage(
        &self,
        batch: &ContractBatch,
    ) -> Vec<runner_batch::CoverageLineFile> {
        if matches!(
            self.mode.as_str(),
            "supplemental-no-lines"
                | "supplemental-under-threshold"
                | "supplemental-under-threshold-ran"
        ) || (!self.mode.starts_with("supplemental-") && self.mode != "unexpected-supplemental")
        {
            return Vec::new();
        }
        vec![runner_batch::CoverageLineFile {
            path: batch.manifest.manifest.files.validator.clone(),
            executable_lines: vec![1, 2],
            covered_lines: vec![2],
        }]
    }

    fn supplemental_report(&self) -> Option<Value> {
        match self.mode.as_str() {
            "unexpected-supplemental"
            | "supplemental-under-threshold"
            | "supplemental-under-threshold-coverage" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-under-threshold-ran" => Some(json!({
                "executed": true,
                "passed": true,
            })),
            "supplemental-no-lines" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-omitted" => None,
            "supplemental-not-executed" => Some(json!({
                "executed": false,
                "passed": true,
            })),
            "supplemental-passed" | "supplemental-failed" => Some(json!({
                "executed": true,
                "passed": self.mode == "supplemental-passed",
                "error": if self.mode == "supplemental-failed" { "fixture supplemental failure" } else { "" },
                "native_report": {"format": "fixture"},
            })),
            _ => None,
        }
    }

    fn results(&self, batch: &ContractBatch) -> Vec<AdapterResult> {
        let mut results = batch
            .requests
            .iter()
            .map(|request| AdapterResult {
                key: request.key.clone(),
                output: (!matches!(self.mode.as_str(), "no-output" | "case-error"))
                    .then(|| request.input.clone()),
                error: (self.mode == "case-error").then(|| "fixture error".into()),
            })
            .collect::<Vec<_>>();
        if self.mode == "omit-step-results" {
            results.retain(|result| result.key.step_id.is_none());
        }
        match self.mode.as_str() {
            "no-results" => results.clear(),
            "duplicate-results" => {
                if let Some(result) = results.first().cloned() {
                    results.push(result);
                }
            }
            "extra-results" => results.push(AdapterResult {
                key: ExecutionKey {
                    target_id: "unexpected".into(),
                    case_id: "unexpected".into(),
                    step_id: None,
                },
                output: Some(Value::Null),
                error: None,
            }),
            _ => {}
        }
        results
    }

    fn coverage(&self, batch: &ContractBatch) -> Vec<CoverageScope> {
        if self.mode == "no-coverage" {
            return Vec::new();
        }
        let mut scopes = batch
            .requests
            .iter()
            .map(|request| {
                let key = if self.mode == "unrequested-coverage" {
                    ExecutionKey {
                        target_id: "unexpected".into(),
                        case_id: "unexpected".into(),
                        step_id: None,
                    }
                } else {
                    request.key.clone()
                };
                let files =
                    match self.mode.as_str() {
                        "undeclared-coverage" => vec![CoverageFile {
                            path: "outside.py".into(),
                            lines: vec![1],
                        }],
                        "invalid-coverage-path" => vec![CoverageFile {
                            path: "../escape.py".into(),
                            lines: vec![1],
                        }],
                        "zero-coverage-line" => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![0],
                        }],
                        "empty-coverage" => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: Vec::new(),
                        }],
                        "multi-file-coverage" => std::iter::once(CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        })
                        .chain(batch.manifest.manifest.files.support.iter().map(|path| {
                            CoverageFile {
                                path: path.clone(),
                                lines: vec![1, 2],
                            }
                        }))
                        .collect(),
                        _ => vec![CoverageFile {
                            path: batch.manifest.manifest.files.validator.clone(),
                            lines: vec![1],
                        }],
                    };
                CoverageScope { key, files }
            })
            .collect::<Vec<_>>();
        if self.mode == "duplicate-coverage" {
            if let Some(scope) = scopes.first().cloned() {
                scopes.push(scope);
            }
        }
        scopes
    }
}

#[test]
fn harness_setup_error_paths_are_unit_only() {
    let adapter = FixtureAdapter {
        mode: "normal".into(),
    };

    let mismatch = case(
        json!({"operation": "dataset-read-error"}),
        json!({"error_contains": "not-the-observed-error"}),
    );
    assert!(validate_dataset_entry_with_adapter(&mismatch, &adapter).is_err());

    let unsafe_validator = case(
        json!({"validator_name": "../validator.py"}),
        json!({"passed": true}),
    );
    assert!(validate_dataset_entry_with_adapter(&unsafe_validator, &adapter).is_err());

    let default_dataset = case(json!({}), json!({"error_contains": ""}));
    assert!(validate_dataset_entry_with_adapter(&default_dataset, &adapter).is_ok());

    let custom_manifest = case(
        json!({"manifest_yaml": "not: [valid"}),
        json!({"error_contains": ""}),
    );
    assert!(validate_dataset_entry_with_adapter(&custom_manifest, &adapter).is_ok());
}

fn case(input: Value, expected: Value) -> DatasetCase {
    serde_json::from_value(json!({
        "id": "unit-fixture",
        "input": input,
        "expected": expected,
    }))
    .expect("test case should deserialize")
}
