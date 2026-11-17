use runner_manifest::{resolve_filesystem_path, validate_package_path, ManifestPlan};
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

/// Validate a language support package using the same loader used to stage
/// its runtime resources for commands.
pub fn validate_language_package(repository: &Path, language: &str) -> Result<(), String> {
    backend::validate_language_package(repository, language)
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
        Self::create_with_runtime_using(root, isolation_runtime, &mut |path| fs::File::create(path))
    }

    fn create_with_runtime_using(
        root: &ManifestPlan,
        isolation_runtime: Option<PathBuf>,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
    ) -> Result<Self, String> {
        Self::create_tree_using(root, true, isolation_runtime, create_file)
    }

    pub fn create_for_backend(
        root: &ManifestPlan,
        backend: Backend,
        isolation_runtime: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::create_for_backend_using(root, backend, isolation_runtime, &mut |path| {
            fs::File::create(path)
        })
    }

    fn create_for_backend_using(
        root: &ManifestPlan,
        backend: Backend,
        isolation_runtime: Option<PathBuf>,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
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
        Self::create_tree_using(root, tempdir_mode, isolation_runtime, create_file)
    }

    fn create_tree_using(
        root: &ManifestPlan,
        tempdir_mode: bool,
        isolation_runtime: Option<PathBuf>,
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
    ) -> Result<Self, String> {
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
            .unwrap_or_else(|| root.package_root.clone());
        let mut tree = Self {
            _temp: temp,
            root: root_dir,
            root_plan: root.clone(),
            read: Vec::new(),
            write: Vec::new(),
            manifests: Vec::new(),
            isolation_runtime,
            tempdir_mode,
        };
        tree.stage_recursive(root, Path::new(""), &mut Vec::new(), create_file)?;
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
        create_file: &mut dyn FnMut(&Path) -> std::io::Result<fs::File>,
    ) -> Result<(), String> {
        if stack.contains(&plan.manifest_path) {
            return Err(format!(
                "nested contract cycle detected: {}",
                plan.manifest_path.display()
            ));
        }
        stack.push(plan.manifest_path.clone());
        self.manifests.push((plan.clone(), prefix.to_path_buf()));
        for entry in &plan.manifest.filesystem.read {
            let source = resolve_filesystem_path(&plan.package_root, entry, true)?;
            if self.tempdir_mode {
                // `.` declares the whole package readable. It is accepted by
                // the filesystem policy resolver, so preserve that meaning
                // when copying the package into a tempdir as well.
                let target_rel = if entry == "." {
                    prefix.to_path_buf()
                } else {
                    validate_package_path(entry)?;
                    prefix.join(entry)
                };
                copy_entry(&source, &plan.package_root, &self.root.join(&target_rel))?;
                self.read
                    .push(if entry == "." && prefix.as_os_str().is_empty() {
                        ".".to_owned()
                    } else {
                        target_rel.to_string_lossy().replace('\\', "/")
                    });
            } else {
                self.read.push(source.to_string_lossy().into_owned());
            }
        }
        for entry in &plan.manifest.filesystem.write {
            if self.tempdir_mode {
                validate_package_path(entry)?;
                let target_rel = prefix.join(entry);
                let staged_target = self.root.join(&target_rel);
                if staged_target.extension().is_some() {
                    let parent = staged_target
                        .parent()
                        .map_or(self.root.as_path(), |path| path);
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    let _created =
                        create_declared_write_file_if_missing(&staged_target, create_file)?;
                } else {
                    fs::create_dir_all(&staged_target).map_err(|e| e.to_string())?;
                }
                self.write
                    .push(target_rel.to_string_lossy().replace('\\', "/"));
            } else {
                let target = resolve_filesystem_path(&plan.package_root, entry, false)?;
                self.write.push(target.to_string_lossy().into_owned());
            }
        }
        for child_name in &plan.manifest.files.nested_contracts {
            validate_package_path(child_name)?;
            let child_source = plan.package_root.join(child_name);
            let child = ManifestPlan::load(&child_source)?;
            let child_rel = Path::new(child_name);
            let child_target = self.root.join(prefix).join(child_rel);
            if self.tempdir_mode {
                copy_entry(&child_source, &plan.package_root, &child_target)?;
                self.read
                    .push(prefix.join(child_rel).to_string_lossy().replace('\\', "/"));
            } else {
                self.read
                    .push(child.manifest_path.to_string_lossy().into_owned());
            }
            let child_prefix = prefix.join(child_rel.parent().unwrap_or(Path::new("")));
            self.stage_recursive(&child, &child_prefix, stack, create_file)?;
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
        language_package: Option<&str>,
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
            language_package,
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
        language_package: Option<&str>,
    ) -> Result<Output, String> {
        self.prepare_direct_write_paths()?;
        let plan = self.plan_command_at(backend, working_directory, command, language_package)?;
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
        language_package: Option<&str>,
    ) -> Result<StagedCommandPlan, String> {
        let root = self
            .root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let working_directory = backend::checked_working_directory(&root, working_directory)?;
        let plan = backend::plan_command(
            backend,
            command,
            language_package,
            self.isolation_runtime.as_deref(),
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
