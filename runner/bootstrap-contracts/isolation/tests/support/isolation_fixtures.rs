use crate::validator::validate_case;
use bootstrap_dataset_harness::DatasetCase;
use runner_manifest::ManifestPlan;
use serde_json::{json, Value};
use std::path::PathBuf;

#[test]
fn isolation_filesystem_and_command_edges_are_unit_fixtures() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan = ManifestPlan::load(package.join("isolation-package.fixture.yaml"))
        .expect("load isolation package fixture");
    let fixtures: Vec<DatasetCase> = vec![
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
