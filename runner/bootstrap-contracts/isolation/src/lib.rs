use runner_manifest::{resolve_filesystem_path, resolve_nested_manifest_path, ManifestPlan};
mod backend;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    TempDir,
    None,
    Bubblewrap,
    Landlock,
}

pub struct StagedCommandPlan {
    pub command: Vec<String>,
    pub working_directory: PathBuf,
    pub runtime_roots: Vec<PathBuf>,
    pub resolved_executable: Option<PathBuf>,
    runtime_plan: backend::CommandPlan,
}

impl Backend {
    pub fn runtime_manifest_name(self) -> Option<&'static str> {
        match self {
            Self::TempDir => None,
            Self::None => None,
            Self::Bubblewrap => Some("bwrap"),
            Self::Landlock => Some("landlock"),
        }
    }
}

/// Choose the strongest isolation backend that can actually start a minimal
/// process in the current environment. Contract execution failures are not
/// retried under weaker backends; this probes backend availability only.
pub fn auto_backend(isolation_runtime: Option<&Path>) -> (Backend, Vec<String>) {
    let mut attempts = Vec::new();
    match backend::probe_bubblewrap(isolation_runtime) {
        Ok(()) => return (Backend::Bubblewrap, attempts),
        Err(error) => attempts.push(format!("bwrap unavailable: {error}")),
    }
    match backend::probe_landlock(isolation_runtime) {
        Ok(()) => return (Backend::Landlock, attempts),
        Err(error) => attempts.push(format!("landlock unavailable: {error}")),
    }
    match tempfile::tempdir() {
        Ok(directory) => {
            drop(directory);
            (Backend::TempDir, attempts)
        }
        Err(error) => {
            attempts.push(format!("tempdir unavailable: {error}"));
            (Backend::None, attempts)
        }
    }
}

/// A contract tree in either a copied temporary workspace or its source
/// checkout. Nested contracts share the root workspace.
pub struct StagedTree {
    _temp: Option<TempDir>,
    pub root: PathBuf,
    source_root: PathBuf,
    pub root_plan: ManifestPlan,
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub manifests: Vec<(ManifestPlan, PathBuf)>,
    isolation_runtime: Option<PathBuf>,
    tempdir_mode: bool,
}

impl StagedTree {
    pub fn create(root: &ManifestPlan) -> Result<Self, String> {
        Self::create_with_runtime(root, None)
    }

    /// Create a staged tree using an optional caller-selected isolation
    /// runtime manifest. When omitted, the selected backend loads its bundled
    /// runtime manifest at command execution time.
    pub fn create_with_runtime(
        root: &ManifestPlan,
        isolation_runtime: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::create_with_runtime_using(
            root,
            isolation_runtime,
            &mut |path| fs::File::create(path),
            &mut |path| fs::create_dir_all(path),
        )
    }

    fn create_with_runtime_using(
        root: &ManifestPlan,
        isolation_runtime: Option<PathBuf>,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
        create_directory: &mut dyn FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<Self, String> {
        Self::create_tree_using(root, true, isolation_runtime, true, create_file, create_directory)
    }

    pub fn create_for_backend(
        root: &ManifestPlan,
        backend: Backend,
        isolation_runtime: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::create_for_backend_with_nested_contracts(root, backend, isolation_runtime, true)
    }

    /// Create a staged tree and optionally include contracts declared by the
    /// root and its descendants.
    pub fn create_for_backend_with_nested_contracts(
        root: &ManifestPlan,
        backend: Backend,
        isolation_runtime: Option<PathBuf>,
        include_nested_contracts: bool,
    ) -> Result<Self, String> {
        Self::create_for_backend_using(
            root,
            backend,
            isolation_runtime,
            include_nested_contracts,
            &mut |path| fs::File::create(path),
            &mut |path| fs::create_dir_all(path),
        )
    }

    fn create_for_backend_using(
        root: &ManifestPlan,
        backend: Backend,
        isolation_runtime: Option<PathBuf>,
        include_nested_contracts: bool,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
        create_directory: &mut dyn FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<Self, String> {
        let tempdir_mode = backend == Backend::TempDir;
        if matches!(backend, Backend::None | Backend::TempDir) && isolation_runtime.is_some() {
            return Err(format!(
                "--isolation-runtime cannot be used with --isolation {}",
                if backend == Backend::TempDir {
                    "tempdir"
                } else {
                    "none"
                }
            ));
        }
        Self::create_tree_using(
            root,
            tempdir_mode,
            isolation_runtime,
            include_nested_contracts,
            create_file,
            create_directory,
        )
    }

    fn create_tree_using(
        root: &ManifestPlan,
        tempdir_mode: bool,
        isolation_runtime: Option<PathBuf>,
        include_nested_contracts: bool,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
        create_directory: &mut dyn FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<Self, String> {
        let mut plans = Vec::new();
        collect_contract_plans(
            root,
            include_nested_contracts,
            &mut Vec::new(),
            &mut std::collections::HashSet::new(),
            &mut plans,
        )?;
        let package_roots = plans
            .iter()
            .map(|plan: &ManifestPlan| plan.package_root.clone())
            .collect::<Vec<_>>();
        if let Some(path) = package_roots.iter().find(|path| !path.is_absolute()) {
            return Err(format!(
                "contract package root must be absolute after manifest resolution: {}",
                path.display()
            ));
        }
        let source_root =
            common_ancestor(&root.package_root, package_roots.iter().skip(1).cloned());
        let temp = if tempdir_mode {
            Some(
                tempfile::Builder::new()
                    .prefix("open-contract-")
                    .tempdir()
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let root_dir = temp
            .as_ref()
            .map(|temp| temp.path().to_path_buf())
            .unwrap_or_else(|| source_root.clone());
        let mut tree = Self {
            _temp: temp,
            root: root_dir,
            source_root,
            root_plan: root.clone(),
            read: Vec::new(),
            write: Vec::new(),
            manifests: Vec::new(),
            isolation_runtime,
            tempdir_mode,
        };
        for plan in &plans {
            tree.stage_plan(plan, create_file, create_directory)?;
        }
        tree.read.sort();
        tree.read.dedup();
        tree.write.sort();
        tree.write.dedup();
        Ok(tree)
    }

    fn stage_plan(
        &mut self,
        plan: &ManifestPlan,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
        create_directory: &mut dyn FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<(), String> {
        let prefix = plan
            .package_root
            .strip_prefix(&self.source_root)
            .map_err(|_| "nested contract package is outside the shared source root".to_string())?
            .to_path_buf();
        self.manifests.push((plan.clone(), prefix.to_path_buf()));
        for entry in &plan.manifest.filesystem.read {
            let source = resolve_filesystem_path(&plan.package_root, entry, true)?;
            if self.tempdir_mode {
                let target_rel = source.strip_prefix(&self.source_root).map_err(|_| {
                    format!("filesystem read path is outside the staged source root: {entry}")
                })?;
                copy_entry(&source, &self.source_root, &self.root.join(target_rel))?;
                self.read
                    .push(target_rel.to_string_lossy().replace('\\', "/"));
            } else {
                self.read.push(source.to_string_lossy().into_owned());
            }
        }
        for entry in &plan.manifest.filesystem.write {
            let target = resolve_filesystem_path(&plan.package_root, entry, false)?;
            if self.tempdir_mode {
                let target_rel = target.strip_prefix(&self.source_root).map_err(|_| {
                    format!("filesystem write path is outside the staged source root: {entry}")
                })?;
                let staged_target = self.root.join(&target_rel);
                let is_directory = target.is_dir()
                    || entry.ends_with('/')
                    || (!target.exists() && staged_target.extension().is_none());
                if is_directory {
                    create_directory(&staged_target).map_err(|e| e.to_string())?;
                } else {
                    let parent = staged_target
                        .parent()
                        .map_or(self.root.as_path(), |path| path);
                    create_directory(parent).map_err(|e| e.to_string())?;
                    let _created =
                        create_declared_write_file_if_missing(&staged_target, create_file)?;
                }
                self.write
                    .push(target_rel.to_string_lossy().replace('\\', "/"));
            } else {
                self.write.push(target.to_string_lossy().into_owned());
            }
        }
        Ok(())
    }

    pub fn run_command(
        &self,
        backend: Backend,
        package_root: &Path,
        command: &[String],
        input: &[u8],
    ) -> Result<Output, String> {
        self.prepare_direct_write_paths()?;
        backend::run(
            backend,
            package_root,
            package_root,
            &self.read,
            &self.write,
            command,
            input,
            self.isolation_runtime.as_deref(),
            self.tempdir_mode,
        )
    }

    /// Run a command from a contract package directory while enforcing the
    /// shared staged tree's read and write policy.
    pub fn run_command_at(
        &self,
        backend: Backend,
        working_directory: &Path,
        command: &[String],
        input: &[u8],
    ) -> Result<Output, String> {
        self.prepare_direct_write_paths()?;
        let plan = self.plan_command_at(backend, working_directory, command)?;
        backend::run_prepared(
            backend,
            &self.root,
            &plan.working_directory,
            &self.read,
            &self.write,
            plan.runtime_plan,
            input,
            self.tempdir_mode,
        )
    }

    fn prepare_direct_write_paths(&self) -> Result<(), String> {
        if self.tempdir_mode {
            return Ok(());
        }
        for path in &self.write {
            let path = Path::new(path);
            if path.extension().is_some() {
                let parent = path.parent().unwrap_or(Path::new("."));
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                if !path.exists() {
                    fs::File::create(path).map_err(|error| error.to_string())?;
                }
            } else {
                fs::create_dir_all(path).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    /// Resolve the same command and runtime resources used by execution,
    /// without starting the command.
    pub fn plan_command_at(
        &self,
        backend: Backend,
        working_directory: &Path,
        command: &[String],
    ) -> Result<StagedCommandPlan, String> {
        let root = self
            .root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let working_directory = backend::checked_working_directory(&root, working_directory)?;
        let language_adapter = self
            .manifests
            .iter()
            .find(|(_, prefix)| {
                self.root.join(prefix).canonicalize().ok().as_deref()
                    == Some(working_directory.host.as_path())
            })
            .and_then(|(manifest, _)| manifest.language_adapter.as_deref());
        let plan = backend::plan_command_with_adapter(
            backend,
            command,
            self.isolation_runtime.as_deref(),
            language_adapter,
        )?;
        Ok(StagedCommandPlan {
            command: plan.command.clone(),
            working_directory: working_directory.host,
            runtime_roots: plan.runtime_roots.clone(),
            resolved_executable: plan.resolved_executable.clone(),
            runtime_plan: plan,
        })
    }
}

fn collect_contract_plans(
    plan: &ManifestPlan,
    include_nested_contracts: bool,
    stack: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<PathBuf>,
    plans: &mut Vec<ManifestPlan>,
) -> Result<(), String> {
    if stack.contains(&plan.manifest_path) {
        return Err(format!(
            "nested contract cycle detected: {}",
            plan.manifest_path.display()
        ));
    }
    if !visited.insert(plan.manifest_path.clone()) {
        return Ok(());
    }
    stack.push(plan.manifest_path.clone());
    plans.push(plan.clone());
    if include_nested_contracts {
        for child_name in &plan.manifest.nested_contracts {
            let child_path = resolve_nested_manifest_path(&plan.package_root, child_name)?;
            let child = ManifestPlan::load(child_path)?;
            collect_contract_plans(&child, include_nested_contracts, stack, visited, plans)?;
        }
    }
    stack.pop();
    Ok(())
}

fn common_ancestor(first: &Path, paths: impl Iterator<Item = PathBuf>) -> PathBuf {
    let mut common = first.to_path_buf();
    for path in paths {
        while !path.starts_with(&common) {
            common.pop();
        }
    }
    common
}

fn create_declared_write_file_if_missing(
    target: &Path,
    create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
) -> Result<Option<fs::File>, String> {
    if target.exists() {
        return Ok(None);
    }
    create_file(target)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn copy_entry(source: &Path, package_root: &Path, destination: &Path) -> Result<(), String> {
    copy_entry_with(source, package_root, destination, &mut |directory| {
        fs::read_dir(directory).map(|entries| {
            entries
                .map(|entry| entry.map(|entry| (entry.file_name(), entry.path())))
                .collect::<Vec<_>>()
        })
    })
}

fn copy_entry_with(
    source: &Path,
    package_root: &Path,
    destination: &Path,
    read_directory: &mut dyn FnMut(
        &Path,
    ) -> std::io::Result<
        Vec<std::io::Result<(std::ffi::OsString, PathBuf)>>,
    >,
) -> Result<(), String> {
    let resolved = source.canonicalize().map_err(|e| {
        let display_path = source.strip_prefix(package_root).unwrap_or(source);
        format!("{}: {e}", display_path.display())
    })?;
    let root = package_root.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err(format!(
            "staging source escapes package: {}",
            source.display()
        ));
    }
    if resolved.is_dir() {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in read_directory(&resolved).map_err(|e| e.to_string())? {
            let (file_name, entry) = entry.map_err(|e| e.to_string())?;
            copy_entry_with(&entry, &root, &destination.join(file_name), read_directory)?;
        }
    } else {
        let parent = destination.parent().map_or(package_root, |path| path);
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::copy(resolved, destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
extern crate self as runner_isolation;

#[cfg(test)]
pub(crate) mod test_fixture {
    pub(crate) fn exercise_landlock_installation_paths() -> Result<(), String> {
        super::backend_unit_tests::exercise_landlock_installation_paths()
    }

    pub(crate) fn exercise_backend_error_paths() -> Result<(), String> {
        super::backend_unit_tests::exercise_backend_error_paths()
    }

    pub(crate) fn exercise_staging_copy_error_paths() -> Result<(), String> {
        super::staging_unit_tests::exercise_copy_error_paths()
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../tests/support/backend_unit_tests.rs"]
mod backend_unit_tests;

#[cfg(test)]
#[path = "../tests/support/staging_unit_tests.rs"]
mod staging_unit_tests;

#[cfg(test)]
#[path = "../tests/unit_dataset_runner.rs"]
mod dataset_runner_test;
