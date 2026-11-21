use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub name: String,
    pub datasets: Vec<String>,
    pub coverage_sources: Vec<String>,
    #[serde(default)]
    pub supplemental_tests: Vec<String>,
    #[serde(default)]
    pub nested_contracts: Vec<String>,
    #[serde(default)]
    pub directories: BTreeMap<String, String>,
    #[serde(default)]
    pub coverage: Coverage,
    #[serde(default)]
    pub execution: Option<ExecutionCommand>,
    #[serde(default)]
    pub language_adapter: Option<String>,
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
    pub language_adapter: Option<PathBuf>,
}

impl ManifestPlan {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::load_path(path.as_ref())
    }

    fn load_path(path: &Path) -> Result<Self, String> {
        let manifest_path = canonicalize_path(path)?;
        let mut package_root = manifest_path.clone();
        package_root.pop();
        let mut manifest: Manifest =
            serde_yaml::from_slice(&fs::read(&manifest_path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("invalid manifest: {e}"))?;
        if manifest.version != "1" {
            return Err("manifest version must be '1'".into());
        }
        if manifest.name.trim().is_empty() {
            return Err("manifest name must be non-empty".into());
        }
        validate_directory_aliases(&package_root, &manifest.directories)?;
        for path in &mut manifest.datasets {
            *path = resolve_directory_alias(path, &manifest.directories)?;
        }
        for path in &mut manifest.supplemental_tests {
            *path = resolve_directory_alias(path, &manifest.directories)?;
        }
        for path in &mut manifest.nested_contracts {
            *path = resolve_directory_alias(path, &manifest.directories)?;
        }
        for path in &mut manifest.filesystem.read {
            *path = resolve_directory_alias(path, &manifest.directories)?;
        }
        for path in &mut manifest.filesystem.write {
            *path = resolve_directory_alias(path, &manifest.directories)?;
        }
        manifest.coverage_sources = resolve_coverage_sources(
            &package_root,
            &manifest.coverage_sources,
            &manifest.directories,
        )?;
        if !manifest.coverage.minimum_line_percent.is_finite()
            || !(0.0..=100.0).contains(&manifest.coverage.minimum_line_percent)
        {
            return Err("coverage.minimum_line_percent must be between 0 and 100".into());
        }
        if let Some(execution) = &manifest.execution {
            if execution.command.is_empty() || execution.command[0].trim().is_empty() {
                return Err("manifest execution command must not be empty".into());
            }
        }
        let language_adapter = manifest
            .language_adapter
            .as_deref()
            .map(|path| resolve_filesystem_path(&package_root, path, true))
            .transpose()?;
        if let Some(path) = &language_adapter {
            if !path.is_file() {
                return Err(format!(
                    "language_adapter is not a regular file: {}",
                    path.display()
                ));
            }
        }
        if manifest.datasets.is_empty() {
            return Err("manifest datasets must include at least one path".into());
        }
        let mut declared_files = manifest
            .datasets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        declared_files.extend(manifest.coverage_sources.iter().map(String::as_str));
        declared_files.extend(manifest.supplemental_tests.iter().map(String::as_str));
        let resolved_read_paths = manifest
            .filesystem
            .read
            .iter()
            .map(|path| resolve_filesystem_path(&package_root, path, true))
            .collect::<Result<Vec<_>, _>>()?;
        for nested_manifest in &manifest.nested_contracts {
            let resolved = resolve_nested_manifest_path(&package_root, nested_manifest)?;
            if !resolved_read_paths
                .iter()
                .any(|read_path| resolved.starts_with(read_path))
            {
                return Err(format!(
                    "nested contract manifest is not readable: {nested_manifest}"
                ));
            }
        }
        for path in &manifest.filesystem.write {
            resolve_filesystem_path(&package_root, path, false)?;
        }
        for path in declared_files {
            validate_package_path(path)?;
            let resolved = resolve_inside(&package_root, path)?;
            if !resolved.is_file() {
                return Err(format!("manifest file is not a regular file: {path}"));
            }
            if !resolved_read_paths
                .iter()
                .any(|read_path| resolved.starts_with(read_path))
            {
                return Err(format!("manifest file is not readable: {path}"));
            }
        }
        Ok(Self {
            manifest,
            manifest_path,
            package_root,
            language_adapter,
        })
    }
}

/// Resolve a nested contract manifest relative to the manifest that declares it.
/// Nested contracts may live beside or above the declaring package, but the
/// reference itself must remain relative and identify a regular file.
pub fn resolve_nested_manifest_path(package_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if raw.is_empty()
        || raw.contains('\0')
        || raw.contains('\\')
        || raw.as_bytes().get(1) == Some(&b':')
    {
        return Err(format!(
            "nested contract manifest path must be relative: {raw:?}"
        ));
    }
    let leading_root_component = usize::from(raw.starts_with('/'));
    if raw
        .split('/')
        .skip(leading_root_component)
        .any(|component| component.is_empty())
    {
        return Err(format!(
            "nested contract manifest path has an empty component: {raw:?}"
        ));
    }
    let mut candidate = package_root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                candidate.push(name);
                match fs::symlink_metadata(&candidate) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(format!(
                            "nested contract manifest path traverses a symlink: {raw}"
                        ));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(format!(
                            "cannot inspect nested contract manifest {raw:?}: {error}"
                        ));
                    }
                }
            }
            Component::ParentDir => {
                candidate.pop();
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "nested contract manifest path must be relative: {raw:?}"
                ));
            }
        }
    }
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve nested contract manifest {raw:?}: {error}"))?;
    if !resolved.is_file() {
        return Err(format!(
            "nested contract manifest is not a regular file: {raw}"
        ));
    }
    Ok(resolved)
}

fn validate_directory_aliases(
    package_root: &Path,
    aliases: &BTreeMap<String, String>,
) -> Result<(), String> {
    for (name, raw_path) in aliases {
        let mut characters = name.chars();
        let valid_name = characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            });
        if !valid_name {
            return Err(format!("invalid directory alias name: {name:?}"));
        }
        validate_package_path(raw_path)?;
        let resolved = resolve_inside(package_root, raw_path)?;
        if !resolved.is_dir() {
            return Err(format!(
                "directory alias @{name} does not name a directory: {raw_path}"
            ));
        }
    }
    Ok(())
}

fn resolve_directory_alias(
    raw: &str,
    aliases: &BTreeMap<String, String>,
) -> Result<String, String> {
    if !raw.starts_with('@') {
        return Ok(raw.to_owned());
    }
    let reference = &raw[1..];
    let (name, suffix) = reference.split_once('/').unwrap_or((reference, ""));
    let root = aliases
        .get(name)
        .ok_or_else(|| format!("unknown directory alias: @{name}"))?;
    if suffix.is_empty() {
        Ok(root.clone())
    } else {
        Ok(format!("{root}/{suffix}"))
    }
}

fn resolve_coverage_sources(
    package_root: &Path,
    declarations: &[String],
    aliases: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let mut resolved = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for declaration in declarations {
        let pattern = resolve_directory_alias(declaration, aliases)?;
        validate_package_path(&pattern)?;
        let absolute_pattern = package_root.join(&pattern).to_string_lossy().into_owned();
        let mut matches = glob::glob(&absolute_pattern)
            .map_err(|error| format!("invalid coverage_sources glob {pattern:?}: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot expand coverage_sources glob {pattern:?}: {error}"))?;
        matches.sort();
        if matches.is_empty() {
            return Err(format!(
                "coverage_sources entry matched no files: {pattern}"
            ));
        }
        for matched in matches {
            let canonical = matched.canonicalize().map_err(|error| error.to_string())?;
            if !canonical.starts_with(package_root) {
                return Err(format!(
                    "coverage_sources path escapes its package: {pattern}"
                ));
            }
            if !canonical.is_file() {
                return Err(format!(
                    "coverage_sources entry is not a regular file: {pattern}"
                ));
            }
            // `pattern` is a validated package-relative path and `matched`
            // came from globbing `package_root.join(pattern)`, so removing
            // the package root is guaranteed by construction. The canonical
            // path check above separately rejects symlink escapes.
            let relative = matched
                .components()
                .skip(package_root.components().count())
                .collect::<PathBuf>()
                .to_string_lossy()
                .replace('\\', "/");
            if seen.insert(relative.clone()) {
                resolved.push(relative);
            }
        }
    }
    Ok(resolved)
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

/// Resolve a filesystem policy entry relative to a contract's source directory,
/// or accept an absolute host path. Unlike files named by `files`, filesystem
/// entries may intentionally refer to shared files outside the contract.
pub fn resolve_filesystem_path(
    package_root: &Path,
    raw: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.contains('\0') {
        return Err(format!("invalid filesystem path: {raw:?}"));
    }
    let path = Path::new(raw);
    let explicitly_external = path.is_absolute() || relative_path_leaves_package(path);
    if must_exist {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            package_root.join(path)
        };
        let resolved = candidate
            .canonicalize()
            .map_err(|error| format!("cannot resolve filesystem path {raw:?}: {error}"))?;
        if !explicitly_external && !resolved.starts_with(package_root) {
            return Err(format!(
                "filesystem path escapes its package through a symlink: {raw}"
            ));
        }
        return Ok(resolved);
    }

    let mut ancestor = if path.is_absolute() {
        PathBuf::new()
    } else {
        package_root.to_path_buf()
    };
    let mut missing_components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => ancestor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if missing_components.pop().is_none() {
                    ancestor.pop();
                }
            }
            Component::Normal(name) => {
                if !missing_components.is_empty() {
                    missing_components.push(name.to_os_string());
                    continue;
                }
                let next = ancestor.join(name);
                match fs::symlink_metadata(&next) {
                    Ok(_) => {
                        ancestor = next.canonicalize().map_err(|error| {
                            format!("cannot resolve filesystem path {raw:?}: {error}")
                        })?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing_components.push(name.to_os_string());
                    }
                    Err(error) => {
                        return Err(format!("cannot inspect filesystem path {raw:?}: {error}"));
                    }
                }
            }
        }
    }
    let mut resolved = ancestor;
    for component in missing_components {
        resolved.push(component);
    }
    if !explicitly_external && !resolved.starts_with(package_root) {
        return Err(format!(
            "filesystem path escapes its package through a symlink: {raw}"
        ));
    }
    Ok(resolved)
}

fn relative_path_leaves_package(path: &Path) -> bool {
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                if depth == 0 {
                    return true;
                }
                depth -= 1;
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    false
}

pub fn is_allowed(path: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|entry| {
        path == entry
            || path
                .strip_prefix(entry)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn resolve_inside(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let candidate = root.join(raw);
    let resolved = candidate
        .canonicalize()
        .map_err(|e| format!("{raw}: {e}"))?;
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

    const VALID: &str = "version: '1'\nname: sample\ndatasets: [dataset.json]\ncoverage_sources: [validator.py]\nfilesystem: {read: [dataset.json, validator.py], write: [reports/deep]}\n";

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
        let plan = ManifestPlan::load(&temp.path().join("contract.yaml")).unwrap();
        assert_eq!(plan.manifest.name, "sample");
        assert!(plan
            .package_root
            .join("reports/deep")
            .starts_with(&plan.package_root));

        for invalid in [
            VALID.replace("version: '1'", "version: '2'"),
            VALID.replace("name: sample", "name: '  '"),
            VALID.replace("read: [dataset.json, validator.py]", "read: [missing.json]"),
            VALID.replace("read: [dataset.json, validator.py]", "read: [dataset.json]"),
            VALID.replace("datasets: [dataset.json]", "datasets: [missing.json]"),
            VALID
                .replace("datasets: [dataset.json]", "datasets: [../escape]")
                .replace("read: [dataset.json, validator.py]", "read: [validator.py]"),
            "not: [valid: yaml".to_string(),
        ] {
            write_manifest(temp.path(), &invalid);
            assert!(ManifestPlan::load(&temp.path().join("contract.yaml")).is_err());
        }

        let directory = temp.path().join("dataset-dir");
        std::fs::create_dir(&directory).unwrap();
        let directory_manifest = VALID
            .replace("datasets: [dataset.json]", "datasets: [dataset-dir]")
            .replace(
                "read: [dataset.json, validator.py]",
                "read: [dataset-dir, validator.py]",
            );
        write_manifest(temp.path(), &directory_manifest);
        assert!(ManifestPlan::load(&temp.path().join("contract.yaml")).is_err());

        assert!(ManifestPlan::load(&temp.path().join("missing.yaml")).is_err());
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
            assert!(ManifestPlan::load(&temp.path().join("contract.yaml")).is_err());

            let write_escape =
                VALID.replace("write: [reports/deep]", "write: [outside-dir/new-file]");
            write_manifest(temp.path(), &write_escape);
            assert!(ManifestPlan::load(&temp.path().join("contract.yaml")).is_err());
        }
    }

    #[test]
    fn coverage_threshold_defaults_to_100_and_rejects_out_of_range_values() {
        let temp = package();
        write_manifest(temp.path(), VALID);
        let plan = ManifestPlan::load(&temp.path().join("contract.yaml")).unwrap();
        assert_eq!(plan.manifest.coverage.minimum_line_percent, 100.0);

        write_manifest(
            temp.path(),
            &VALID.replace(
                "datasets: [dataset.json]",
                "datasets: [dataset.json]\ncoverage: {minimum_line_percent: 101}",
            ),
        );
        assert!(ManifestPlan::load(&temp.path().join("contract.yaml"))
            .unwrap_err()
            .contains("coverage.minimum_line_percent"));
    }

    #[test]
    fn coverage_sources_keep_literals_and_expand_globs_in_sorted_order() {
        let temp = package();
        let source_dir = temp.path().join("src");
        std::fs::create_dir_all(source_dir.join("internal")).unwrap();
        std::fs::write(source_dir.join("z.rs"), "pub fn z() {}\n").unwrap();
        std::fs::write(source_dir.join("a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(source_dir.join("internal/helper.rs"), "fn helper() {}\n").unwrap();
        let manifest = "version: '1'\nname: sources\ndatasets: [dataset.json]\ncoverage_sources: [src/**/*.rs]\nfilesystem: {read: [dataset.json, src], write: []}\n";
        write_manifest(temp.path(), manifest);

        let plan = ManifestPlan::load(&temp.path().join("contract.yaml")).unwrap();
        assert_eq!(
            plan.manifest.coverage_sources,
            vec!["src/a.rs", "src/internal/helper.rs", "src/z.rs"]
        );
    }

    #[test]
    fn coverage_sources_include_only_declared_literal_sources() {
        let temp = package();
        let source_dir = temp.path().join("src");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(source_dir.join("public.rs"), "pub fn api() {}\n").unwrap();
        std::fs::write(source_dir.join("helper.rs"), "fn helper() {}\n").unwrap();
        let manifest = "version: '1'\nname: sources\ndatasets: [dataset.json]\ncoverage_sources: [src/public.rs]\nfilesystem: {read: [dataset.json, src], write: []}\n";
        write_manifest(temp.path(), manifest);

        let plan = ManifestPlan::load(&temp.path().join("contract.yaml")).unwrap();
        assert_eq!(plan.manifest.coverage_sources, vec!["src/public.rs"]);
    }

    #[test]
    fn coverage_source_glob_must_match_at_least_one_file() {
        let temp = package();
        let manifest = "version: '1'\nname: sources\ndatasets: [dataset.json]\ncoverage_sources: [src/**/*.rs]\nfilesystem: {read: [dataset.json, src], write: []}\n";
        std::fs::create_dir(temp.path().join("src")).unwrap();
        write_manifest(temp.path(), manifest);

        let error = ManifestPlan::load(&temp.path().join("contract.yaml")).unwrap_err();
        assert!(error.contains("coverage_sources entry matched no files"));
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
