use serde::Deserialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub name: String,
    pub files: Files,
    #[serde(default)]
    pub coverage: Coverage,
    #[serde(default)]
    pub execution: Option<ExecutionCommand>,
    pub filesystem: FileSystem,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    #[serde(default = "default_minimum_line_percent")]
    pub minimum_line_percent: f64,
}

impl Default for Coverage {
    fn default() -> Self {
        Self {
            minimum_line_percent: default_minimum_line_percent(),
        }
    }
}

fn default_minimum_line_percent() -> f64 {
    100.0
}

/// Language adapter command that executes one validator request. Dataset and
/// step iteration remain in the Rust runner.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCommand {
    pub command: Vec<String>,
    #[serde(default)]
    pub runtime_profile: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Files {
    pub dataset: String,
    pub validator: String,
    #[serde(default)]
    pub support: Vec<String>,
    #[serde(default)]
    pub supplemental_tests: Vec<String>,
    #[serde(default)]
    pub nested_contracts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSystem {
    pub read: Vec<String>,
    pub write: Vec<String>,
}

/// A parsed manifest plus its source location. This layer does not select or
/// configure an isolation backend.
#[derive(Clone, Debug)]
pub struct ManifestPlan {
    pub manifest: Manifest,
    pub manifest_path: PathBuf,
    pub package_root: PathBuf,
}

impl ManifestPlan {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let manifest_path = canonicalize_path(path.as_ref())?;
        let package_root = manifest_path
            .parent()
            .ok_or_else(|| "canonical manifest path has no parent directory".to_string())?
            .to_path_buf();
        let manifest: Manifest =
            serde_yaml::from_slice(&fs::read(&manifest_path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("invalid manifest: {e}"))?;
        if manifest.version != "1" {
            return Err("manifest version must be '1'".into());
        }
        if manifest.name.trim().is_empty() {
            return Err("manifest name must be non-empty".into());
        }
        if !manifest.coverage.minimum_line_percent.is_finite()
            || !(0.0..=100.0).contains(&manifest.coverage.minimum_line_percent)
        {
            return Err("coverage.minimum_line_percent must be between 0 and 100".into());
        }
        if let Some(execution) = &manifest.execution {
            if execution.command.is_empty() || execution.command[0].trim().is_empty() {
                return Err("manifest execution command must not be empty".into());
            }
            if execution
                .runtime_profile
                .as_ref()
                .is_some_and(|profile| profile.trim().is_empty())
            {
                return Err("manifest execution runtime profile must not be empty".into());
            }
        }
        let mut declared_files = vec![
            manifest.files.dataset.as_str(),
            manifest.files.validator.as_str(),
        ];
        declared_files.extend(manifest.files.support.iter().map(String::as_str));
        declared_files.extend(manifest.files.supplemental_tests.iter().map(String::as_str));
        declared_files.extend(manifest.files.nested_contracts.iter().map(String::as_str));
        for path in &manifest.filesystem.read {
            validate_package_path(path)?;
            resolve_inside(&package_root, path, true)?;
        }
        for path in &manifest.filesystem.write {
            validate_package_path(path)?;
            resolve_inside(&package_root, path, false)?;
        }
        for path in declared_files {
            validate_package_path(path)?;
            let resolved = resolve_inside(&package_root, path, true)?;
            if !resolved.is_file() {
                return Err(format!("manifest file is not a regular file: {path}"));
            }
            if !is_allowed(path, &manifest.filesystem.read) {
                return Err(format!("manifest file is not readable: {path}"));
            }
        }
        Ok(Self {
            manifest,
            manifest_path,
            package_root,
        })
    }
}

pub fn validate_package_path(raw: &str) -> Result<(), String> {
    if raw.contains('\\') || Path::new(raw).is_absolute() {
        return Err(format!("manifest path must be package-relative: {raw:?}"));
    }
    let mut parts = Path::new(raw).components();
    let first = parts
        .next()
        .ok_or_else(|| format!("manifest path has no components: {raw:?}"))?;
    if !matches!(first, Component::Normal(_))
        || raw
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(format!("manifest path must stay within the package: {raw}"));
    }
    if raw.split('/').next().is_some_and(|part| part.contains(':')) {
        return Err(format!(
            "manifest path must not contain a drive prefix: {raw}"
        ));
    }
    Ok(())
}

pub fn is_allowed(path: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|entry| {
        path == entry
            || path
                .strip_prefix(entry)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn resolve_inside(root: &Path, raw: &str, must_exist: bool) -> Result<PathBuf, String> {
    let candidate = root.join(raw);
    let resolved = if must_exist {
        candidate
            .canonicalize()
            .map_err(|e| format!("{raw}: {e}"))?
    } else {
        let mut ancestor = candidate.as_path();
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| format!("cannot resolve a package-relative ancestor for {raw:?}"))?;
        }
        let real_ancestor = canonicalize_path(ancestor)?;
        let suffix = candidate
            .strip_prefix(ancestor)
            .map_err(|error| format!("cannot resolve package path {raw:?}: {error}"))?;
        real_ancestor.join(suffix)
    };
    if !resolved.starts_with(root) {
        return Err(format!(
            "manifest path escapes its package through a symlink: {raw}"
        ));
    }
    Ok(resolved)
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_path, is_allowed, validate_package_path, ManifestPlan};
    use std::path::Path;

    fn package() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("dataset.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("validator.py"),
            "def validate(x): return x\n",
        )
        .unwrap();
        temp
    }

    fn write_manifest(root: &Path, text: &str) {
        std::fs::write(root.join("contract.yaml"), text).unwrap();
    }

    const VALID: &str = "version: '1'\nname: sample\nfiles: {dataset: dataset.json, validator: validator.py}\nfilesystem: {read: [dataset.json, validator.py], write: [reports/deep]}\n";

    #[test]
    fn package_paths_reject_escape_and_absolute_paths() {
        assert!(validate_package_path("nested/data.json").is_ok());
        assert!(validate_package_path("../secret").is_err());
        assert!(validate_package_path("/outside/private.txt").is_err());
        assert!(validate_package_path("").is_err());
        assert!(validate_package_path("a\\b").is_err());
        assert!(validate_package_path("a//b").is_err());
        assert!(validate_package_path("a/./b").is_err());
        assert!(validate_package_path("C:/secret").is_err());
        assert!(canonicalize_path(Path::new("missing-path-for-canonicalization")).is_err());
    }

    #[test]
    fn read_allowlist_matches_path_components() {
        let allowed = vec!["data".to_string()];
        assert!(is_allowed("data", &allowed));
        assert!(is_allowed("data/input.json", &allowed));
        assert!(!is_allowed("database/input.json", &allowed));
        assert!(!is_allowed("elsewhere", &allowed));
    }

    #[test]
    fn manifest_plan_loads_valid_packages_and_rejects_invalid_boundaries() {
        let temp = package();
        write_manifest(temp.path(), VALID);
        let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
        assert_eq!(plan.manifest.name, "sample");
        assert!(plan
            .package_root
            .join("reports/deep")
            .starts_with(&plan.package_root));

        for invalid in [
            VALID.replace("version: '1'", "version: '2'"),
            VALID.replace("name: sample", "name: '  '"),
            VALID.replace("read: [dataset.json, validator.py]", "read: [/etc/passwd]"),
            VALID.replace("read: [dataset.json, validator.py]", "read: [missing.json]"),
            VALID.replace("write: [reports/deep]", "write: [/tmp/out]"),
            VALID.replace("read: [dataset.json, validator.py]", "read: [dataset.json]"),
            VALID.replace("dataset: dataset.json", "dataset: missing.json"),
            VALID
                .replace("dataset: dataset.json", "dataset: ../escape")
                .replace("read: [dataset.json, validator.py]", "read: [validator.py]"),
            "not: [valid: yaml".to_string(),
        ] {
            write_manifest(temp.path(), &invalid);
            assert!(ManifestPlan::load(temp.path().join("contract.yaml")).is_err());
        }

        let directory = temp.path().join("dataset-dir");
        std::fs::create_dir(&directory).unwrap();
        let directory_manifest = VALID
            .replace("dataset: dataset.json", "dataset: dataset-dir")
            .replace(
                "read: [dataset.json, validator.py]",
                "read: [dataset-dir, validator.py]",
            );
        write_manifest(temp.path(), &directory_manifest);
        assert!(ManifestPlan::load(temp.path().join("contract.yaml")).is_err());

        assert!(ManifestPlan::load(temp.path().join("missing.yaml")).is_err());
        assert!(ManifestPlan::load(temp.path()).is_err());

        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret"), "secret").unwrap();
            std::os::unix::fs::symlink(outside.path(), temp.path().join("outside-dir")).unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret"), temp.path().join("escape"))
                .unwrap();
            let symlink_manifest = VALID.replace("dataset.json", "escape");
            write_manifest(temp.path(), &symlink_manifest);
            assert!(ManifestPlan::load(temp.path().join("contract.yaml")).is_err());

            let write_escape =
                VALID.replace("write: [reports/deep]", "write: [outside-dir/new-file]");
            write_manifest(temp.path(), &write_escape);
            assert!(ManifestPlan::load(temp.path().join("contract.yaml")).is_err());
        }
    }

    #[test]
    fn coverage_threshold_defaults_to_100_and_rejects_out_of_range_values() {
        let temp = package();
        write_manifest(temp.path(), VALID);
        let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
        assert_eq!(plan.manifest.coverage.minimum_line_percent, 100.0);

        write_manifest(
            temp.path(),
            &VALID.replace(
                "files: {dataset: dataset.json, validator: validator.py}",
                "files: {dataset: dataset.json, validator: validator.py}\ncoverage: {minimum_line_percent: 101}",
            ),
        );
        assert!(ManifestPlan::load(temp.path().join("contract.yaml"))
            .unwrap_err()
            .contains("coverage.minimum_line_percent"));
    }

    #[test]
    fn manifest_contract_dataset_exercises_path_validation() {
        let dataset: serde_yaml::Value =
            serde_yaml::from_str(include_str!("../dataset.yaml")).unwrap();
        let case = &dataset["cases"][0];
        let path = case["input"]["path"].as_str().unwrap();
        validate_package_path(path).unwrap();
        assert_eq!(case["expected"]["valid"].as_bool(), Some(true));
    }
}
