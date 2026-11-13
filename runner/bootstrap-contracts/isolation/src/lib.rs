use open_contract_manifest::{validate_package_path, ManifestPlan};
mod backend;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    None,
    Bubblewrap,
    Landlock,
}

/// Validate a language runtime profile using the same loader used to stage
/// runtime resources for commands.
pub fn validate_runtime_profile(repository: &Path, language: &str) -> Result<(), String> {
    backend::validate_runtime_profile(repository, language)
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bubblewrap => "bwrap",
            Self::Landlock => "landlock",
        }
    }
}

/// One randomized host staging tree shared by the root contract and all its
/// declared nested contracts. Runtime backend is provided only at execution.
pub struct StagedTree {
    _temp: TempDir,
    pub root: PathBuf,
    pub root_plan: ManifestPlan,
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub manifests: Vec<ManifestPlan>,
    manifest_prefixes: Vec<PathBuf>,
}

impl StagedTree {
    pub fn create(root: &ManifestPlan) -> Result<Self, String> {
        let temp = tempfile::Builder::new()
            .prefix("open-contract-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let root_dir = temp.path().to_path_buf();
        let mut tree = Self {
            _temp: temp,
            root: root_dir,
            root_plan: root.clone(),
            read: Vec::new(),
            write: Vec::new(),
            manifests: Vec::new(),
            manifest_prefixes: Vec::new(),
        };
        tree.stage_recursive(root, Path::new(""), &mut Vec::new())?;
        tree.read.sort();
        tree.read.dedup();
        tree.write.sort();
        tree.write.dedup();
        Ok(tree)
    }

    fn stage_recursive(
        &mut self,
        plan: &ManifestPlan,
        prefix: &Path,
        stack: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if stack.contains(&plan.manifest_path) {
            return Err(format!(
                "nested contract cycle detected: {}",
                plan.manifest_path.display()
            ));
        }
        stack.push(plan.manifest_path.clone());
        self.manifests.push(plan.clone());
        self.manifest_prefixes.push(prefix.to_path_buf());
        for entry in &plan.manifest.filesystem.read {
            let source = plan.package_root.join(entry);
            let target_rel = prefix.join(entry);
            copy_entry(&source, &plan.package_root, &self.root.join(&target_rel))?;
            self.read
                .push(target_rel.to_string_lossy().replace('\\', "/"));
        }
        for entry in &plan.manifest.filesystem.write {
            let target_rel = prefix.join(entry);
            let target = self.root.join(&target_rel);
            if target.extension().is_some() {
                let parent = target.parent().map_or(self.root.as_path(), |path| path);
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                if !target.exists() {
                    fs::File::create(&target).map_err(|e| e.to_string())?;
                }
            } else {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            }
            self.write
                .push(target_rel.to_string_lossy().replace('\\', "/"));
        }
        for child_name in &plan.manifest.files.nested_contracts {
            validate_package_path(child_name)?;
            let child_source = plan.package_root.join(child_name);
            let child = ManifestPlan::load(&child_source)?;
            let child_rel = Path::new(child_name);
            let child_target = self.root.join(prefix).join(child_rel);
            copy_entry(&child_source, &plan.package_root, &child_target)?;
            self.read
                .push(prefix.join(child_rel).to_string_lossy().replace('\\', "/"));
            let child_prefix = prefix.join(child_rel.parent().unwrap_or(Path::new("")));
            self.stage_recursive(&child, &child_prefix, stack)?;
        }
        stack.pop();
        Ok(())
    }

    pub fn run_command(
        &self,
        backend: Backend,
        package_root: &Path,
        command: &[String],
        input: &[u8],
        runtime_profile: Option<&str>,
    ) -> Result<Output, String> {
        if command.is_empty() {
            return Err("isolation command cannot be empty".into());
        }
        backend::run(
            backend,
            package_root,
            &self.read,
            &self.write,
            command,
            input,
            runtime_profile,
        )
    }

    pub fn root_manifest(&self) -> Option<&ManifestPlan> {
        self.manifests.first()
    }

    pub fn manifests_with_prefixes(&self) -> impl Iterator<Item = (&ManifestPlan, &Path)> {
        self.manifests
            .iter()
            .zip(&self.manifest_prefixes)
            .map(|(manifest, prefix)| (manifest, prefix.as_path()))
    }
}

fn copy_entry(source: &Path, package_root: &Path, destination: &Path) -> Result<(), String> {
    let resolved = source
        .canonicalize()
        .map_err(|e| format!("{}: {e}", source.display()))?;
    let root = package_root.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err(format!(
            "staging source escapes package: {}",
            source.display()
        ));
    }
    if resolved.is_dir() {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&resolved).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_entry(&entry.path(), &root, &destination.join(entry.file_name()))?;
        }
    } else {
        let parent = destination.parent().map_or(package_root, |path| path);
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::copy(resolved, destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Backend, StagedTree};
    use open_contract_manifest::ManifestPlan;
    use std::path::Path;

    fn package(temp: &Path, name: &str, filesystem: &str, files: &str) -> ManifestPlan {
        std::fs::write(temp.join("dataset.json"), "{\"version\":1,\"cases\":[]}").unwrap();
        std::fs::write(
            temp.join("validator.py"),
            "def validate(value): return value\n",
        )
        .unwrap();
        std::fs::write(
            temp.join("contract.yaml"),
            format!("version: '1'\nname: {name}\nfiles: {{dataset: dataset.json, validator: validator.py{files}}}\nfilesystem: {filesystem}\n"),
        ).unwrap();
        ManifestPlan::load(temp.join("contract.yaml")).unwrap()
    }

    #[test]
    fn isolation_contract_dataset_is_staged_under_a_fresh_root() {
        let dataset: serde_json::Value =
            serde_json::from_str(include_str!("../dataset.json")).unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("dataset.json"), dataset.to_string()).unwrap();
        std::fs::write(
            temp.path().join("validator.py"),
            "def validate(value): return value\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("contract.yaml"),
            "version: '1'\nname: isolation-fixture\nfiles: {dataset: dataset.json, validator: validator.py}\nfilesystem: {read: [dataset.json, validator.py], write: [reports]}\n",
        )
        .unwrap();
        let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
        let first = StagedTree::create(&plan).unwrap();
        let second = StagedTree::create(&plan).unwrap();
        assert_ne!(first.root, second.root);
        let staged: serde_json::Value =
            serde_json::from_slice(&std::fs::read(first.root.join("dataset.json")).unwrap())
                .unwrap();
        assert_eq!(
            staged["cases"][0]["input"]["value"],
            dataset["cases"][0]["input"]["value"]
        );
        assert_eq!(Backend::None.as_str(), "none");
        assert_eq!(Backend::Bubblewrap.as_str(), "bwrap");
        assert_eq!(Backend::Landlock.as_str(), "landlock");
        assert!(first.root_manifest().is_some());
    }

    #[test]
    fn isolation_stages_write_files_and_executes_a_command() {
        let temp = tempfile::tempdir().unwrap();
        let plan = package(
            temp.path(),
            "write-fixture",
            "{read: [dataset.json, validator.py], write: [reports/result.json]}",
            "",
        );
        let tree = StagedTree::create(&plan).unwrap();
        assert!(tree.root.join("reports/result.json").is_file());
        let output = tree
            .run_command(
                Backend::None,
                &tree.root,
                &[
                    "python3".into(),
                    "-c".into(),
                    "import sys; print(sys.stdin.read())".into(),
                ],
                b"input-check",
                None,
            )
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "input-check"
        );
        assert!(tree
            .run_command(Backend::None, &tree.root, &[], b"", None)
            .is_err());
    }

    #[test]
    fn isolation_rejects_nested_cycles_and_package_escape_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let cycle = package(
            temp.path(),
            "cycle",
            "{read: [dataset.json, validator.py, contract.yaml], write: []}",
            ", nested_contracts: [contract.yaml]",
        );
        assert!(StagedTree::create(&cycle)
            .err()
            .unwrap()
            .contains("cycle detected"));

        #[cfg(unix)]
        {
            let temp = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret"), "secret").unwrap();
            std::fs::create_dir(temp.path().join("escape")).unwrap();
            std::fs::write(temp.path().join("escape/child"), "inside").unwrap();
            std::fs::write(temp.path().join("dataset.json"), "{}").unwrap();
            std::fs::write(
                temp.path().join("validator.py"),
                "def validate(value): return value\n",
            )
            .unwrap();
            std::fs::write(
                temp.path().join("contract.yaml"),
                "version: '1'\nname: symlink-escape\nfiles: {dataset: dataset.json, validator: validator.py}\nfilesystem: {read: [dataset.json, validator.py, escape], write: []}\n",
            ).unwrap();
            let plan = ManifestPlan::load(temp.path().join("contract.yaml")).unwrap();
            std::fs::remove_dir_all(temp.path().join("escape")).unwrap();
            std::os::unix::fs::symlink(outside.path(), temp.path().join("escape")).unwrap();
            assert!(StagedTree::create(&plan)
                .err()
                .unwrap()
                .contains("escapes package"));
        }
    }
}
