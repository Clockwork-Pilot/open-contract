use serde::Deserialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub name: String,
    pub files: Files,
    pub filesystem: FileSystem,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Files {
    pub dataset: String,
    pub validator: String,
    #[serde(default)]
    pub support: Vec<String>,
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
        let manifest_path = path.as_ref().canonicalize().map_err(|e| e.to_string())?;
        let package_root = manifest_path
            .parent()
            .ok_or_else(|| "manifest has no parent directory".to_string())?
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
        let mut declared_files = vec![
            manifest.files.dataset.as_str(),
            manifest.files.validator.as_str(),
        ];
        declared_files.extend(manifest.files.support.iter().map(String::as_str));
        declared_files.extend(manifest.files.nested_contracts.iter().map(String::as_str));
        for path in &manifest.filesystem.read {
            validate_package_path(path)?;
            let resolved = resolve_inside(&package_root, path, true)?;
            if !resolved.exists() {
                return Err(format!("manifest read path does not exist: {path}"));
            }
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
    if raw.is_empty() || raw.contains('\\') || Path::new(raw).is_absolute() {
        return Err(format!("manifest path must be package-relative: {raw:?}"));
    }
    let mut parts = Path::new(raw).components();
    let first = parts
        .next()
        .ok_or_else(|| "manifest path is empty".to_string())?;
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
                .ok_or_else(|| format!("invalid write path: {raw}"))?;
        }
        let real_ancestor = ancestor.canonicalize().map_err(|e| e.to_string())?;
        let suffix = candidate
            .strip_prefix(ancestor)
            .map_err(|e| e.to_string())?;
        real_ancestor.join(suffix)
    };
    if !resolved.starts_with(root) {
        return Err(format!(
            "manifest path escapes its package through a symlink: {raw}"
        ));
    }
    Ok(resolved)
}
