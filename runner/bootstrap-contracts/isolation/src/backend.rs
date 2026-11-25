use super::Backend;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

pub(super) fn run(
    backend: Backend,
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    argv: &[String],
    input: &[u8],
    isolation_runtime: Option<&Path>,
    tempdir_mode: bool,
) -> Result<Output, String> {
    let plan = plan_command(backend, argv, isolation_runtime)?;
    run_prepared(
        backend,
        package_root,
        working_directory,
        read,
        write,
        plan,
        input,
        tempdir_mode,
    )
}

pub(super) fn probe_bubblewrap(isolation_runtime: Option<&Path>) -> Result<(), String> {
    probe_isolated_command(ProbeBackend::Bubblewrap, isolation_runtime)
}

pub(super) fn probe_landlock(isolation_runtime: Option<&Path>) -> Result<(), String> {
    probe_isolated_command(ProbeBackend::Landlock, isolation_runtime)
}

#[derive(Clone, Copy)]
enum ProbeBackend {
    Bubblewrap,
    Landlock,
}

impl ProbeBackend {
    fn backend(self) -> Backend {
        match self {
            Self::Bubblewrap => Backend::Bubblewrap,
            Self::Landlock => Backend::Landlock,
        }
    }
}

fn probe_isolated_command(
    backend: ProbeBackend,
    isolation_runtime: Option<&Path>,
) -> Result<(), String> {
    let executable = find_executable("true").ok_or_else(|| {
        "could not find a `true` executable for the availability probe".to_string()
    })?;
    let argv = vec![executable.to_string_lossy().into_owned()];
    let plan = plan_command(backend.backend(), &argv, isolation_runtime)?;
    let package = tempfile::tempdir().map_err(|error| error.to_string())?;
    let output = match backend {
        ProbeBackend::Bubblewrap => run_bubblewrap(
            package.path(),
            package.path(),
            &[],
            &[],
            &plan.command,
            &[],
            &plan.runtime,
            false,
            package.path(),
        )?,
        ProbeBackend::Landlock => run_landlock(
            package.path(),
            package.path(),
            &[],
            &[],
            &plan.command,
            &[],
            &plan.runtime,
            package.path(),
        )?,
    };
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("probe command exited with {}", output.status)
        } else {
            stderr
        })
    }
}

pub(super) fn run_prepared(
    backend: Backend,
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    plan: CommandPlan,
    input: &[u8],
    tempdir_mode: bool,
) -> Result<Output, String> {
    let runtime = plan.runtime;
    let argv = plan.command;
    let command_tmp = tempfile::Builder::new()
        .prefix("open-contract-command-")
        .tempdir()
        .map_err(|error| format!("cannot create contract command temporary directory: {error}"))?;
    match backend {
        Backend::TempDir | Backend::None => {
            let root = package_root
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let cwd = checked_working_directory(&root, working_directory)?;
            let mut environment = runtime.environment.clone();
            set_temp_environment(&mut environment, command_tmp.path());
            let mut command = command(&argv, &cwd.host, &environment);
            collect_output(&mut command, input)
        }
        Backend::Bubblewrap => run_bubblewrap(
            package_root,
            working_directory,
            read,
            write,
            &argv,
            input,
            &runtime,
            tempdir_mode,
            command_tmp.path(),
        ),
        Backend::Landlock => run_landlock(
            package_root,
            working_directory,
            read,
            write,
            &argv,
            input,
            &runtime,
            command_tmp.path(),
        ),
    }
}

pub struct CommandPlan {
    pub command: Vec<String>,
    pub runtime_roots: Vec<PathBuf>,
    pub resolved_executable: Option<PathBuf>,
    pub(super) runtime: Runtime,
}

pub(super) fn plan_command(
    backend: Backend,
    argv: &[String],
    isolation_runtime: Option<&Path>,
) -> Result<CommandPlan, String> {
    plan_command_with_adapter(backend, argv, isolation_runtime, None)
}

pub(super) fn plan_command_with_adapter(
    backend: Backend,
    argv: &[String],
    isolation_runtime: Option<&Path>,
    language_adapter: Option<&Path>,
) -> Result<CommandPlan, String> {
    let prepared = prepare_command(backend, argv, isolation_runtime, language_adapter)?;
    Ok(CommandPlan {
        command: prepared.command,
        runtime_roots: prepared.runtime.roots.clone(),
        resolved_executable: prepared.resolved_executable,
        runtime: prepared.runtime,
    })
}

struct PreparedCommand {
    command: Vec<String>,
    runtime: Runtime,
    resolved_executable: Option<PathBuf>,
}

fn prepare_command(
    backend: Backend,
    argv: &[String],
    isolation_runtime: Option<&Path>,
    language_adapter: Option<&Path>,
) -> Result<PreparedCommand, String> {
    let mut runtime = runtime(backend, isolation_runtime)?;
    if let Some(manifest_path) = language_adapter {
        add_language_adapter_runtime(&mut runtime, manifest_path)?;
    }
    let command = expand_command(argv)?;
    if command
        .first()
        .is_none_or(|executable| executable.trim().is_empty())
    {
        return Err(
            "execution.command executable cannot be empty after environment expansion".into(),
        );
    }
    let resolved_executable = if runtime.command_executable {
        let execution_environment = safe_environment(&runtime.environment);
        let search_path = execution_environment.get("PATH").map(String::as_str);
        let executable = find_executable_in_path(&command[0], search_path).ok_or_else(|| {
            format!(
                "isolation runtime permits the command executable, but it cannot be resolved: {}",
                command[0]
            )
        })?;
        runtime
            .roots
            .extend(runtime_command_roots_for_executable(&executable));
        runtime.roots.sort();
        runtime.roots.dedup();
        Some(executable)
    } else {
        None
    };
    Ok(PreparedCommand {
        command,
        runtime,
        resolved_executable,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageAdapterRuntimeManifest {
    version: u32,
    entrypoint: PathBuf,
    files: Vec<PathBuf>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    environment_paths: BTreeMap<String, PathBuf>,
}

fn add_language_adapter_runtime(runtime: &mut Runtime, manifest_path: &Path) -> Result<(), String> {
    let manifest_path = manifest_path.canonicalize().map_err(|error| {
        format!("cannot resolve language adapter runtime manifest {}: {error}", manifest_path.display())
    })?;
    let package_root = manifest_path
        .parent()
        .unwrap_or(manifest_path.as_path());
    let bytes = fs::read(&manifest_path).map_err(|error| {
        format!("cannot read language adapter runtime manifest {}: {error}", manifest_path.display())
    })?;
    let manifest: LanguageAdapterRuntimeManifest = serde_yaml::from_slice(&bytes).map_err(|error| {
        format!("invalid language adapter runtime manifest {}: {error}", manifest_path.display())
    })?;
    if manifest.version != 1 {
        return Err(format!("unsupported language adapter runtime manifest version {}", manifest.version));
    }
    if manifest.files.is_empty() {
        return Err("language adapter runtime manifest declares no files".into());
    }
    let resources = manifest.files.iter()
        .map(|path| checked_adapter_path(package_root, path))
        .collect::<Result<Vec<_>, _>>()?;
    let entrypoint = checked_adapter_path(package_root, &manifest.entrypoint)?;
    if !entrypoint.is_file() || !resources.iter().any(|root| entrypoint.starts_with(root)) {
        return Err("language adapter entrypoint must be a file included in its declared files".into());
    }
    runtime.environment.insert(
        "OPEN_CONTRACT_LANGUAGE_ENTRYPOINT".into(),
        entrypoint.to_string_lossy().into_owned(),
    );
    for (key, value) in manifest.environment_paths {
        let path = checked_adapter_path(package_root, &value)?;
        if !resources.iter().any(|root| path.starts_with(root)) {
            return Err(format!("language adapter environment path {} is outside declared files", value.display()));
        }
        runtime.environment.insert(key, path.to_string_lossy().into_owned());
    }
    runtime.environment.extend(manifest.environment);
    runtime.roots.extend(resources);
    Ok(())
}

fn checked_adapter_path(package_root: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() || path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err(format!("language adapter runtime paths must stay inside the runtime manifest directory: {}", path.display()));
    }
    let resolved = package_root.join(path).canonicalize().map_err(|error| {
        format!("language adapter runtime path {} is unavailable: {error}", path.display())
    })?;
    if !resolved.starts_with(package_root) {
        return Err(format!("language adapter runtime path escapes its package: {}", path.display()));
    }
    Ok(resolved)
}

/// Expand explicit `${NAME}` references in command arguments. Values remain
/// one argv element; this deliberately does not invoke a shell or split words.
fn expand_command(argv: &[String]) -> Result<Vec<String>, String> {
    argv.iter()
        .map(|argument| expand_argument(argument))
        .collect()
}

fn expand_argument(argument: &str) -> Result<String, String> {
    let mut expanded = String::with_capacity(argument.len());
    let mut remaining = argument;
    while let Some(start) = remaining.find("${") {
        expanded.push_str(&remaining[..start]);
        let reference = &remaining[start + 2..];
        let end = reference.find('}').ok_or_else(|| {
            format!("unterminated environment reference in command argument {argument:?}")
        })?;
        let name = &reference[..end];
        let mut characters = name.chars();
        let valid_start = characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
        if !valid_start
            || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            return Err(format!(
                "invalid environment variable name {name:?} in command argument {argument:?}"
            ));
        }
        let value = std::env::var_os(name)
            .ok_or_else(|| {
                format!("execution.command references unset environment variable {name}")
            })?
            .into_string()
            .map_err(|_| format!("environment variable {name} is not valid UTF-8"))?;
        expanded.push_str(&value);
        remaining = &reference[end + 1..];
    }
    expanded.push_str(remaining);
    Ok(expanded)
}

pub(super) struct Runtime {
    roots: Vec<PathBuf>,
    environment: BTreeMap<String, String>,
    command_executable: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsolationRuntimeManifest {
    version: u32,
    #[serde(default)]
    read_only: Vec<PathBuf>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    command_executable: bool,
}

pub(super) fn runtime(
    backend: Backend,
    isolation_runtime: Option<&Path>,
) -> Result<Runtime, String> {
    let mut runtime = Runtime {
        roots: Vec::new(),
        environment: BTreeMap::new(),
        command_executable: false,
    };
    if let Some(runtime_name) = backend.runtime_manifest_name() {
        if let Some(path) = isolation_runtime {
            add_isolation_runtime(&mut runtime, path)?;
        } else {
            let manifest = match runtime_name {
                "bwrap" => include_str!("../../../isolation-runtime/bwrap.yaml"),
                "landlock" => include_str!("../../../isolation-runtime/landlock.yaml"),
                _ => unreachable!("Backend only names supported runtime manifests"),
            };
            add_isolation_runtime_contents(
                &mut runtime,
                &format!("embedded {runtime_name} runtime manifest"),
                manifest.as_bytes(),
            )?;
        }
    }
    runtime.roots.sort();
    runtime.roots.dedup();
    Ok(runtime)
}

fn add_isolation_runtime(runtime: &mut Runtime, manifest_path: &Path) -> Result<(), String> {
    let bytes = fs::read(manifest_path).map_err(|error| {
        format!(
            "cannot read isolation runtime manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    add_isolation_runtime_contents(runtime, &manifest_path.display().to_string(), &bytes)
}

fn add_isolation_runtime_contents(
    runtime: &mut Runtime,
    source: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let manifest: IsolationRuntimeManifest = serde_yaml::from_slice(bytes)
        .map_err(|error| format!("invalid isolation runtime manifest {source}: {error}"))?;
    if manifest.version != 1 {
        return Err(format!(
            "unsupported isolation runtime manifest version {}",
            manifest.version
        ));
    }
    for path in manifest.read_only {
        if !path.is_absolute() {
            return Err(format!(
                "isolation runtime paths must be absolute: {}",
                path.display()
            ));
        }
        if !path.exists() {
            return Err(format!(
                "isolation runtime path {} is unavailable",
                path.display()
            ));
        }
        // Preserve Bubblewrap namespace paths such as /lib64 even when they
        // are host symlinks to directories under /usr.
        runtime.roots.push(path);
    }
    runtime.environment.extend(manifest.environment);
    runtime.command_executable = manifest.command_executable;
    Ok(())
}

fn command(argv: &[String], cwd: &Path, environment: &BTreeMap<String, String>) -> Command {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub(super) fn collect_output(command: &mut Command, input: &[u8]) -> Result<Output, String> {
    let mut wait = |child: Child| child.wait_with_output();
    collect_output_with_wait(command, input, &mut wait)
}

pub(super) fn collect_output_with_wait(
    command: &mut Command,
    input: &[u8],
    wait: &mut dyn FnMut(Child) -> io::Result<Output>,
) -> Result<Output, String> {
    let executable = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start command executable {executable}: {error}"))?;
    let write_error = child
        .stdin
        .take()
        .ok_or_else(|| "command stdin unavailable".to_string())?
        .write_all(input)
        .err();
    let output = wait(child).map_err(|error| error.to_string())?;
    if let Some(error) = write_error {
        return Err(format!(
            "failed writing command input: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output)
}

fn run_bubblewrap(
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    argv: &[String],
    input: &[u8],
    runtime: &Runtime,
    tempdir_mode: bool,
    command_tmp: &Path,
) -> Result<Output, String> {
    let bwrap = find_executable("bwrap")
        .ok_or_else(|| "bwrap is not installed or not on PATH".to_string())?;
    let root = package_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let cwd = checked_working_directory(&root, working_directory)?;
    let guest = guest_root(&root, tempdir_mode)?;
    let mut args = vec![
        "--die-with-parent".into(),
        "--new-session".into(),
        "--unshare-all".into(),
        "--clearenv".into(),
    ];
    let mut created = HashSet::from(["/".to_string()]);
    for source in &runtime.roots {
        bind_readonly(&mut args, source, source, &mut created);
    }
    args.extend([
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
    ]);
    args.extend([
        "--tmpfs".into(),
        "/tmp".into(),
        "--dir".into(),
        "/contract".into(),
    ]);
    created.insert("/contract".into());
    created.insert("/tmp".into());
    let guest_tmp = Path::new("/tmp/open-contract-command-tmp");
    args.extend(["--dir".into(), guest_tmp.to_string_lossy().into_owned()]);
    created.insert(guest_tmp.to_string_lossy().into_owned());
    bind_writable(&mut args, command_tmp, guest_tmp, &mut created);
    args.push("--dir".into());
    args.push(guest.clone());
    created.insert(guest.clone());

    let read_sources = read
        .iter()
        .map(|relative| checked_path(&root, relative))
        .collect::<Result<Vec<_>, _>>()?;
    let write_sources = write
        .iter()
        .map(|relative| checked_path(&root, relative))
        .collect::<Result<Vec<_>, _>>()?;
    let guest_reads = read_sources
        .iter()
        .map(|source| guest_path_within_root(&guest, source))
        .collect::<Vec<_>>();
    let guest_writes = write_sources
        .iter()
        .map(|source| guest_path_within_root(&guest, source))
        .collect::<Vec<_>>();
    let guest_cwd = guest_path_within_root(&guest, &cwd);
    for (source, destination) in read_sources.iter().zip(guest_reads) {
        bind_readonly(&mut args, &source.host, &destination, &mut created);
    }
    for (source, destination) in write_sources.iter().zip(guest_writes) {
        bind_writable(&mut args, &source.host, &destination, &mut created);
    }

    let mut child = Command::new(bwrap);
    child.args(args).arg("--chdir").arg(guest_cwd);
    let mut safe_env = safe_environment(&runtime.environment);
    set_temp_environment(&mut safe_env, guest_tmp);
    for (key, value) in &safe_env {
        child.arg("--setenv").arg(key).arg(value);
    }
    child.arg("--");
    for argument in argv {
        child.arg(map_package_argument(argument, &root, &guest));
    }
    child
        .current_dir(&root)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    collect_output(&mut child, input)
}

fn run_landlock(
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    argv: &[String],
    input: &[u8],
    runtime: &Runtime,
    command_tmp: &Path,
) -> Result<Output, String> {
    let root = package_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let cwd = checked_working_directory(&root, working_directory)?;
    let mut allowed = Vec::new();
    for path in read {
        allowed.push((checked_path(&root, path)?.host, false));
    }
    for path in write {
        allowed.push((checked_path(&root, path)?.host, true));
    }
    for path in &runtime.roots {
        allowed.push((path.clone(), false));
    }
    allowed.push((command_tmp.to_path_buf(), true));
    let rules = landlock_rules(&allowed)?;
    let ruleset = create_landlock_ruleset(&rules).map_err(|error| error.to_string())?;
    let mut environment = safe_environment(&runtime.environment);
    set_temp_environment(&mut environment, command_tmp);
    let mut child = command(argv, &cwd.host, &environment);
    unsafe {
        child.pre_exec(landlock_pre_exec(ruleset, install_landlock));
    }
    collect_output(&mut child, input)
}

pub(super) fn landlock_pre_exec(
    ruleset: OwnedFd,
    install: fn(&OwnedFd) -> io::Result<()>,
) -> impl FnMut() -> io::Result<()> + Send + 'static {
    move || install(&ruleset)
}

pub(super) fn checked_working_directory(
    root: &Path,
    working_directory: &Path,
) -> Result<CheckedPath, String> {
    let cwd = working_directory
        .canonicalize()
        .map_err(|error| format!("command working directory is unavailable: {error}"))?;
    let relative = cwd
        .strip_prefix(root)
        .map_err(|_| {
            "command working directory must be inside the staged contract tree".to_string()
        })?
        .to_path_buf();
    if !cwd.is_dir() {
        return Err("command working directory must be inside the staged contract tree".into());
    }
    Ok(CheckedPath {
        host: cwd,
        relative,
    })
}

pub(super) struct CheckedPath {
    pub(super) host: PathBuf,
    relative: PathBuf,
}

fn checked_path(root: &Path, relative: &str) -> Result<CheckedPath, String> {
    let path = Path::new(relative);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let resolved = full
        .canonicalize()
        .map_err(|error| format!("{}: {error}", full.display()))?;
    let guest_relative = match resolved.strip_prefix(root) {
        Ok(path) => path.to_path_buf(),
        Err(_) if path.is_absolute() => resolved.clone(),
        Err(_) => return Err(format!("staged path escapes package root: {relative}")),
    };
    Ok(CheckedPath {
        host: resolved,
        relative: guest_relative,
    })
}

fn guest_root(root: &Path, tempdir_mode: bool) -> Result<String, String> {
    if !tempdir_mode {
        return Ok(root.to_string_lossy().into_owned());
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "staged package has no usable randomized root name".to_string())?;
    Ok(format!("/contract/{name}"))
}

fn guest_path_within_root(guest: &str, source: &CheckedPath) -> PathBuf {
    Path::new(guest).join(&source.relative)
}

fn map_package_argument(argument: &str, root: &Path, guest: &str) -> String {
    let root = root.to_string_lossy();
    if argument == root {
        return guest.to_string();
    }
    argument
        .strip_prefix(&*root)
        .filter(|suffix| suffix.starts_with(std::path::MAIN_SEPARATOR))
        .map(|suffix| format!("{guest}{suffix}"))
        .unwrap_or_else(|| argument.to_string())
}

fn bind_readonly(
    args: &mut Vec<String>,
    source: &Path,
    destination: &Path,
    created: &mut HashSet<String>,
) {
    ensure_parent_dirs(args, destination, created);
    args.extend([
        "--ro-bind".into(),
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    ]);
    created.insert(destination.to_string_lossy().into_owned());
}

fn bind_writable(
    args: &mut Vec<String>,
    source: &Path,
    destination: &Path,
    created: &mut HashSet<String>,
) {
    ensure_parent_dirs(args, destination, created);
    args.extend([
        "--bind".into(),
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    ]);
    created.insert(destination.to_string_lossy().into_owned());
}

fn ensure_parent_dirs(args: &mut Vec<String>, target: &Path, created: &mut HashSet<String>) {
    let mut missing = target
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| path != "/" && !created.contains(path));
    let mut parents = missing.by_ref().collect::<Vec<_>>();
    parents.reverse();
    for parent in parents {
        args.extend(["--dir".into(), parent.clone()]);
        created.insert(parent);
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    find_executable_in_path(
        name,
        std::env::var_os("PATH")
            .as_deref()
            .and_then(|path| path.to_str()),
    )
}

fn find_executable_in_path(name: &str, search_path: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.exists().then(|| path.to_path_buf());
    }
    std::env::split_paths(&std::ffi::OsStr::new(search_path?))
        .map(|dir| dir.join(path))
        .find(|candidate| candidate.is_file())
        .or_else(|| (path.components().count() > 1).then(|| path.to_path_buf()))
}

pub(super) fn runtime_command_roots_for_executable(executable: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(directory) = executable.parent().filter(|_| executable.is_absolute()) {
        roots.push(directory.to_path_buf());
    }
    roots.extend(
        canonicalize_runtime_executable(executable)
            .filter(|_| executable.is_absolute())
            .and_then(|resolved| resolved.parent().map(Path::to_path_buf)),
    );
    roots.sort();
    roots.dedup();
    roots
}

fn safe_environment(runtime: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::from([
        ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ("HOME".into(), "/tmp".into()),
        (
            "LANG".into(),
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()),
        ),
    ]);
    environment.extend(
        runtime
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    environment
}

fn set_temp_environment(environment: &mut BTreeMap<String, String>, path: &Path) {
    let path = path.to_string_lossy().into_owned();
    for key in ["TMPDIR", "TMP", "TEMP"] {
        environment.insert(key.to_owned(), path.clone());
    }
}

#[cfg(target_os = "linux")]
pub(super) type LandlockRules = Vec<(OwnedFd, u64)>;

#[cfg(target_os = "linux")]
pub(super) fn landlock_rules(allowed: &[(PathBuf, bool)]) -> Result<LandlockRules, String> {
    landlock_rules_with_open(allowed, &mut open_landlock_rule_path)
}

#[cfg(target_os = "linux")]
pub(super) fn landlock_rules_with_open(
    allowed: &[(PathBuf, bool)],
    open_rule_path: &mut dyn FnMut(&Path) -> io::Result<OwnedFd>,
) -> Result<LandlockRules, String> {
    const READ_ACCESS: u64 = (1 << 0) | (1 << 2) | (1 << 3);
    const WRITE_ACCESS: u64 = (1 << 1)
        | (1 << 4)
        | (1 << 5)
        | (1 << 6)
        | (1 << 7)
        | (1 << 8)
        | (1 << 9)
        | (1 << 10)
        | (1 << 11)
        | (1 << 12)
        | (1 << 13)
        | (1 << 14);
    const EXECUTE: u64 = 1;
    let mut paths: HashMap<PathBuf, u64> = HashMap::new();
    for (path, writable) in allowed {
        let metadata =
            fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut access = READ_ACCESS;
        if *writable {
            access |= if metadata.is_dir() {
                WRITE_ACCESS
            } else {
                (1 << 1) | (1 << 14)
            };
        }
        if !metadata.is_dir() {
            access &= !(1 << 3);
        }
        paths
            .entry(path.clone())
            .and_modify(|existing| *existing |= access)
            .or_insert(access);
        let mut parent = path.parent();
        while let Some(directory) = parent {
            paths
                .entry(directory.to_path_buf())
                .and_modify(|existing| *existing |= EXECUTE)
                .or_insert(EXECUTE);
            parent = directory.parent();
        }
    }
    paths
        .into_iter()
        .map(|(path, access)| {
            let fd = open_rule_path(&path).map_err(|error| {
                format!("cannot open Landlock rule path {}: {error}", path.display())
            })?;
            Ok((fd, access))
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub(super) fn open_landlock_rule_path(path: &Path) -> io::Result<OwnedFd> {
    let encoded = CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let fd = unsafe { libc::open(encoded.as_ptr(), 0o10000000 | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
#[repr(C)]
pub(super) struct RulesetAttr {
    handled_access_fs: u64,
}

#[cfg(target_os = "linux")]
#[repr(C)]
pub(super) struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

#[cfg(target_os = "linux")]
pub(super) enum LandlockCall<'a> {
    QueryAbi,
    CreateRuleset(&'a RulesetAttr),
    AddPathRule(i32, &'a PathBeneathAttr),
}

#[cfg(target_os = "linux")]
pub(super) fn create_landlock_ruleset(rules: &LandlockRules) -> io::Result<OwnedFd> {
    const CREATE: libc::c_long = 444;
    const ADD: libc::c_long = 445;
    const VERSION: libc::c_int = 1;
    create_landlock_ruleset_with(rules, &mut |call| match call {
        LandlockCall::QueryAbi => unsafe {
            libc::syscall(CREATE, std::ptr::null::<RulesetAttr>(), 0usize, VERSION)
        },
        LandlockCall::CreateRuleset(attr) => unsafe {
            libc::syscall(CREATE, attr, std::mem::size_of::<RulesetAttr>(), 0)
        },
        LandlockCall::AddPathRule(fd, rule) => unsafe { libc::syscall(ADD, fd, 1, rule, 0) },
    })
}

#[cfg(target_os = "linux")]
pub(super) fn create_landlock_ruleset_with(
    rules: &LandlockRules,
    syscall: &mut dyn FnMut(LandlockCall<'_>) -> libc::c_long,
) -> io::Result<OwnedFd> {
    let abi = syscall(LandlockCall::QueryAbi);
    if abi < 1 {
        return Err(io::Error::last_os_error());
    }
    let mut handled = (1u64 << 13) - 1;
    if abi >= 2 {
        handled |= 1 << 13;
    }
    if abi >= 3 {
        handled |= 1 << 14;
    }
    let attr = RulesetAttr {
        handled_access_fs: handled,
    };
    let ruleset = syscall(LandlockCall::CreateRuleset(&attr));
    if ruleset < 0 {
        return Err(io::Error::last_os_error());
    }
    let ruleset = unsafe { OwnedFd::from_raw_fd(ruleset as i32) };
    for (path, access) in rules {
        let rule = PathBeneathAttr {
            allowed_access: *access & handled,
            parent_fd: path.as_raw_fd(),
        };
        if syscall(LandlockCall::AddPathRule(ruleset.as_raw_fd(), &rule)) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(ruleset)
}

#[cfg(target_os = "linux")]
pub(super) fn install_landlock(ruleset: &OwnedFd) -> io::Result<()> {
    const RESTRICT: libc::c_long = 446;
    install_landlock_with(
        ruleset,
        &mut || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) },
        &mut |fd| unsafe { libc::syscall(RESTRICT, fd, 0) },
    )
}

#[cfg(target_os = "linux")]
pub(super) fn install_landlock_with(
    ruleset: &OwnedFd,
    set_no_new_privs: &mut dyn FnMut() -> libc::c_int,
    restrict: &mut dyn FnMut(i32) -> libc::c_long,
) -> io::Result<()> {
    if set_no_new_privs() < 0 {
        return Err(io::Error::last_os_error());
    }
    if restrict(ruleset.as_raw_fd()) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(super) type LandlockRules = Vec<(OwnedFd, u64)>;

#[cfg(not(target_os = "linux"))]
fn landlock_rules(_allowed: &[(PathBuf, bool)]) -> Result<LandlockRules, String> {
    Err("Landlock is available only on Linux".into())
}

#[cfg(not(target_os = "linux"))]
fn create_landlock_ruleset(_rules: &LandlockRules) -> io::Result<OwnedFd> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Landlock is available only on Linux",
    ))
}

#[cfg(not(target_os = "linux"))]
fn install_landlock(_ruleset: &OwnedFd) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Landlock is available only on Linux",
    ))
}

fn canonicalize_runtime_executable(executable: &Path) -> Option<PathBuf> {
    executable
        .is_absolute()
        .then(|| executable.canonicalize().ok())
        .flatten()
}
