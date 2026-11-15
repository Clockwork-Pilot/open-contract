use crate::validator::validate_case;
use bootstrap_dataset_harness::DatasetCase;
use runner_manifest::ManifestPlan;
use serde_json::{json, Value};
use std::path::PathBuf;

#[test]
fn language_package_filesystem_setup_edges_are_unit_fixtures() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan = ManifestPlan::load(package.join("isolation-package.fixture.yaml"))
        .expect("load isolation package fixture");
    let fixtures: Vec<DatasetCase> = vec![
        serde_json::from_value(json!({
    "id": "language-package-resolves-from-runner-configuration",
    "input": {
        "operation": "language-package",
        "language_package": "custom-python-support",
        "external_package_mapping": true,
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: src/runner.py\n    files: [src]\n    environment_paths: {OPEN_CONTRACT_LANGUAGE_PATH: src}\n"
    },
    "expected": {
        "valid": true
    }
})).expect("valid unit fixture"),
    serde_json::from_value(json!({
    "id": "language-package-rejects-invalid-runner-configuration",
    "setup": true,
    "input": {
        "operation": "language-package",
        "language_package": "python",
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: src/runner.py\n    files: [src]\n"
    },
    "expected": {
        "error_contains": "invalid OPEN_CONTRACT_LANGUAGE_PACKAGES JSON"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-reference-must-be-logical",
    "input": {
        "operation": "language-package",
        "language_package": "bad/id",
        "create_runtime_manifest": false
    },
    "expected": {
        "error_contains": "invalid logical language package identifier"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-reports-missing-configured-root",
    "input": {
        "operation": "language-package",
        "language_package": "custom-python-support",
        "external_package_mapping": true,
        "missing_mapping_target": true,
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: src/runner.py\n    files: [src]\n"
    },
    "expected": {
        "error_contains": "is unavailable at"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-missing-manifest",
    "input": {
        "operation": "language-package",
        "language_package": "python",
        "create_runtime_manifest": false
    },
    "expected": {
        "error_contains": "cannot read python runtime manifest"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-entrypoint-outside-resources",
    "input": {
        "operation": "language-package",
        "language_package": "python",
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: outside.py\n    files: [src]\n    environment: {MODE: production}\n    environment_paths: {ADAPTER: src/runner.py}\n",
        "entrypoint_path": "outside.py"
    },
    "expected": {
        "error_contains": "entrypoint must be a file included"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-environment-path-outside-resources",
    "input": {
        "operation": "language-package",
        "language_package": "python",
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: src/runner.py\n    files: [src]\n    environment: {MODE: production}\n    environment_paths: {ADAPTER: outside.py}\n",
        "create_outside_resource": true
    },
    "expected": {
        "error_contains": "environment path \"outside.py\" is outside"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "language-package-symlink-escape",
    "input": {
        "operation": "language-package",
        "language_package": "python",
        "runtime_yaml": "version: 1\nprofiles:\n  production:\n    entrypoint: escape\n    files: [escape]\n    environment: {MODE: production}\n    environment_paths: {ADAPTER: src/runner.py}\n",
        "create_entrypoint": false,
        "create_symlink_escape": true
    },
    "expected": {
        "error_contains": "runtime resource escapes language package"
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "randomized-stage",
    "input": {
        "operation": "stage"
    },
    "expected": {
        "dataset_staged": true,
        "validator_staged": true,
        "support_staged": true,
        "nested_dataset_staged": true,
        "fresh_root": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "reject-nested-cycle-and-package-escape",
    "input": {
        "operation": "reject-cycle-and-package-escape"
    },
    "expected": {
        "cycle_rejected": true,
        "package_escape_rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "reject-invalid-isolation-paths",
    "input": {
        "operation": "reject-invalid-staged-paths"
    },
    "expected": {
        "parent_traversal_rejected": true,
        "symlink_escape_rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "contract-command-runs-from-its-package-directory",
    "input": {
        "operation": "command-at"
    },
    "expected": {
        "working_directory": "nested",
        "outside_rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "unknown-language-package",
    "input": {
        "operation": "unknown-language-package"
    },
    "expected": {
        "rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "missing-command-executable",
    "input": {
        "operation": "command-not-found"
    },
    "expected": {
        "rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "broken-command-stdin-pipe",
    "input": {
        "operation": "broken-stdin-pipe"
    },
    "expected": {
        "rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "landlock-denies-undeclared-source",
    "input": {
        "operation": "deny-private-read"
    },
    "expected": {
        "denied": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "bubblewrap-missing-launcher",
    "input": {
        "operation": "missing-bubblewrap"
    },
    "expected": {
        "rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "bubblewrap-command-builder-through-fake-launcher",
    "input": {
        "operation": "mocked-bubblewrap"
    },
    "expected": {
        "launcher_invoked": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "runtime-mounts-only-language-adapters",
    "input": {
        "operation": "runtime-mounts-only-adapters"
    },
    "expected": {
        "adapter_roots_only": true,
        "javascript_runtime_packages": [
            "contract-languages/javascript/node_modules/v8-to-istanbul",
            "contract-languages/javascript/node_modules/@jridgewell/trace-mapping",
            "contract-languages/javascript/node_modules/@jridgewell/resolve-uri",
            "contract-languages/javascript/node_modules/@jridgewell/sourcemap-codec",
            "contract-languages/javascript/node_modules/convert-source-map"
        ]
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "reject-bubblewrap-root-without-name",
    "input": {
        "operation": "bubblewrap-invalid-root"
    },
    "expected": {
        "rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "bubblewrap-command-or-explicit-namespace-limit",
    "input": {
        "operation": "bwrap-command"
    },
    "expected": {
        "attempted": true,
        "namespace_unavailable": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "empty-command-is-rejected",
    "input": {
        "operation": "empty-command"
    },
    "expected": {
        "rejected": true,
        "package_command_rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "command-environment-references",
    "input": {
        "operation": "command-environment-references"
    },
    "expected": {
        "expanded": true,
        "invalid_references_rejected": true
    }
})).expect("valid unit fixture"),
        serde_json::from_value(json!({
    "id": "isolation-runtime-yaml-source-of-truth",
    "input": {
        "operation": "isolation-runtime-manifest"
    },
    "expected": {
        "override_applied": true,
        "invalid_manifests_rejected": true
    }
})).expect("valid unit fixture"),
    ];
    let package_dataset: Value = serde_json::from_str(include_str!("../../dataset.json"))
        .expect("package dataset should parse");
    let mut previous_root = None;
    for case in fixtures {
        let dataset = if case.input["operation"] == "stage" {
            &package_dataset
        } else {
            &Value::Null
        };
        previous_root = Some(
            validate_case(&plan, &package, dataset, &previous_root, &case)
                .unwrap_or_else(|error| panic!("unit fixture {} failed: {error}", case.id)),
        );
    }
}
