use crate::manifest::{validate_package_path, ManifestPlan};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    None,
    Bubblewrap,
    Landlock,
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
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub manifests: Vec<ManifestPlan>,
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
            read: Vec::new(),
            write: Vec::new(),
            manifests: Vec::new(),
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
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
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
        let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/contract-isolate");
        let mut launcher = Command::new("python3");
        launcher
            .arg(helper)
            .arg("--backend")
            .arg(backend.as_str())
            .arg("--package-root")
            .arg(package_root);
        for path in &self.read {
            launcher.arg("--read").arg(path);
        }
        for path in &self.write {
            launcher.arg("--write").arg(path);
        }
        if let Some(profile) = runtime_profile {
            launcher.arg("--runtime-profile").arg(profile);
        }
        launcher
            .arg("--")
            .args(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = launcher
            .spawn()
            .map_err(|e| format!("failed to start isolation launcher: {e}"))?;
        use std::io::Write;
        child
            .stdin
            .take()
            .ok_or_else(|| "isolation stdin unavailable".to_string())?
            .write_all(input)
            .map_err(|e| e.to_string())?;
        child.wait_with_output().map_err(|e| e.to_string())
    }

    pub fn root_manifest(&self) -> Option<&ManifestPlan> {
        self.manifests.first()
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
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(resolved, destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}
