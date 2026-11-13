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
use std::process::{Command, Output, Stdio};

pub(super) fn run(
    backend: Backend,
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    argv: &[String],
    input: &[u8],
    language_package: Option<&str>,
    isolation_runtime: Option<&Path>,
) -> Result<Output, String> {
    let plan = plan_command(backend, argv, language_package, isolation_runtime)?;
    run_prepared(backend, package_root, working_directory, read, write, plan, input)
}

pub(super) fn run_prepared(
    backend: Backend,
    package_root: &Path,
    working_directory: &Path,
    read: &[String],
    write: &[String],
    plan: CommandPlan,
    input: &[u8],
) -> Result<Output, String> {
    let runtime = plan.runtime;
    let argv = plan.command;
    match backend {
        Backend::None => {
            let root = package_root
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let cwd = checked_working_directory(&root, working_directory)?;
            let mut command = command(&argv, &cwd, &runtime.environment);
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
        ),
        Backend::Landlock => run_landlock(
            package_root,
            working_directory,
            read,
            write,
            &argv,
            input,
            &runtime,
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
    language_package: Option<&str>,
    isolation_runtime: Option<&Path>,
) -> Result<CommandPlan, String> {
    let prepared = prepare_command(backend, argv, language_package, isolation_runtime)?;
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
    language_package: Option<&str>,
    isolation_runtime: Option<&Path>,
) -> Result<PreparedCommand, String> {
    let mut runtime = runtime(language_package, backend, isolation_runtime)?;
    let command = expand_command(argv)?;
    if command
        .first()
        .is_none_or(|executable| executable.trim().is_empty())
    {
        return Err("execution.command executable cannot be empty after environment expansion".into());
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

/// Expand explicit `${NAME}` references in command arguments. Values remain
/// one argv element; this deliberately does not invoke a shell or split words.
fn expand_command(argv: &[String]) -> Result<Vec<String>, String> {
    argv.iter().map(|argument| expand_argument(argument)).collect()
}

fn expand_argument(argument: &str) -> Result<String, String> {
    let mut expanded = String::with_capacity(argument.len());
    let mut remaining = argument;
    while let Some(start) = remaining.find("${") {
        expanded.push_str(&remaining[..start]);
        let reference = &remaining[start + 2..];
        let end = reference
            .find('}')
            .ok_or_else(|| format!("unterminated environment reference in command argument {argument:?}"))?;
        let name = &reference[..end];
        let mut characters = name.chars();
        let valid_start = characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
        if !valid_start
            || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            return Err(format!("invalid environment variable name {name:?} in command argument {argument:?}"));
        }
        let value = std::env::var_os(name)
            .ok_or_else(|| format!("execution.command references unset environment variable {name}"))?
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
struct RuntimeManifest {
    version: u32,
    profiles: BTreeMap<String, RuntimeProfile>,
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

#[derive(Deserialize)]
struct RuntimeProfile {
    entrypoint: String,
    files: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    environment_paths: BTreeMap<String, String>,
}

pub(super) fn validate_language_package(repository: &Path, language: &str) -> Result<(), String> {
    let mut runtime = Runtime {
        roots: Vec::new(),
        environment: BTreeMap::new(),
        command_executable: false,
    };
    add_language_package(&mut runtime, repository, language)
}

fn runtime(
    profile: Option<&str>,
    backend: Backend,
    isolation_runtime: Option<&Path>,
) -> Result<Runtime, String> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|error| format!("runner root is unavailable: {error}"))?;
    let mut runtime = Runtime {
        roots: Vec::new(),
        environment: BTreeMap::new(),
        command_executable: false,
    };
    match profile {
        None => {}
        Some(package) => add_language_package(&mut runtime, &repository, package)?,
    }
    if backend != Backend::None {
        let runtime_manifest = match isolation_runtime {
            Some(path) => path.to_path_buf(),
            None => repository
                .join("runner/isolation-runtime")
                .join(format!("{}.yaml", backend.as_str())),
        };
        add_isolation_runtime(&mut runtime, &runtime_manifest)?;
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
    let manifest: IsolationRuntimeManifest = serde_yaml::from_slice(&bytes).map_err(|error| {
        format!(
            "invalid isolation runtime manifest {}: {error}",
            manifest_path.display()
        )
    })?;
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

fn add_language_package(
    runtime: &mut Runtime,
    repository: &Path,
    language: &str,
) -> Result<(), String> {
    let package = language_package_root(repository, language)?;
    let manifest_path = package.join("runtime.yaml");
    let bytes = fs::read(&manifest_path).map_err(|error| {
        format!(
            "cannot read {language} runtime manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_yaml::from_slice(&bytes).map_err(|error| {
        format!(
            "invalid {language} runtime manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    if manifest.version != 1 {
        return Err(format!(
            "unsupported {language} runtime manifest version {}",
            manifest.version
        ));
    }
    let profile = manifest
        .profiles
        .get("production")
        .ok_or_else(|| format!("{language} runtime manifest has no production profile"))?;
    let package = package
        .canonicalize()
        .map_err(|error| format!("{language} runtime package is unavailable: {error}"))?;
    let resources = profile
        .files
        .iter()
        .map(|path| checked_runtime_path(&package, path))
        .collect::<Result<Vec<_>, _>>()?;
    if resources.is_empty() {
        return Err(format!(
            "{language} production profile declares no runtime files"
        ));
    }
    let entrypoint = checked_runtime_path(&package, &profile.entrypoint)?;
    if !entrypoint.is_file() || !resources.iter().any(|root| entrypoint.starts_with(root)) {
        return Err(format!("{language} production entrypoint must be a file included in its declared runtime files"));
    }
    for (key, value) in &profile.environment {
        runtime.environment.insert(key.clone(), value.clone());
    }
    for (key, value) in &profile.environment_paths {
        let path = checked_runtime_path(&package, value)?;
        if !resources.iter().any(|root| path.starts_with(root)) {
            return Err(format!(
                "{language} environment path {value:?} is outside declared runtime files"
            ));
        }
        runtime
            .environment
            .insert(key.clone(), path.to_string_lossy().into_owned());
    }
    runtime.roots.extend(resources);
    Ok(())
}

fn language_package_root(repository: &Path, package_id: &str) -> Result<PathBuf, String> {
    let mut characters = package_id.chars();
    let valid_id = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    if !valid_id {
        return Err(format!("invalid logical language package identifier: {package_id:?}"));
    }
    if let Some(config) = std::env::var_os("OPEN_CONTRACT_LANGUAGE_PACKAGES") {
        let mappings: BTreeMap<String, String> = serde_json::from_str(&config.to_string_lossy())
            .map_err(|error| format!("invalid OPEN_CONTRACT_LANGUAGE_PACKAGES JSON: {error}"))?;
        if let Some(root) = mappings.get(package_id) {
            return PathBuf::from(root).canonicalize().map_err(|error| {
                format!("language package {package_id:?} is unavailable at {root:?}: {error}")
            });
        }
    }
    repository
        .join("contract-languages")
        .join(package_id)
        .canonicalize()
        .map_err(|error| format!("language package {package_id:?} is unavailable: {error}"))
}

fn checked_runtime_path(package: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(format!("invalid runtime package path: {relative}"));
    }
    let full = package.join(path);
    let resolved = full.canonicalize().map_err(|error| {
        format!(
            "runtime resource {} is unavailable: {error}",
            full.display()
        )
    })?;
    if !resolved.starts_with(package) {
        return Err(format!(
            "runtime resource escapes language package: {relative}"
        ));
    }
    Ok(resolved)
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

fn collect_output(command: &mut Command, input: &[u8]) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let write_error = child
        .stdin
        .take()
        .ok_or_else(|| "command stdin unavailable".to_string())?
        .write_all(input)
        .err();
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
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
) -> Result<Output, String> {
    let bwrap = find_executable("bwrap")
        .ok_or_else(|| "bwrap is not installed or not on PATH".to_string())?;
    let root = package_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let cwd = checked_working_directory(&root, working_directory)?;
    let guest = guest_root(package_root)?;
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
    args.push("--dir".into());
    args.push(guest.clone());
    created.insert(guest.clone());

    for relative in read {
        let source = checked_path(&root, relative)?;
        bind_readonly(
            &mut args,
            &source,
            &guest_path(&guest, &root, &source)?,
            &mut created,
        );
    }
    for relative in write {
        let source = checked_path(&root, relative)?;
        bind_writable(
            &mut args,
            &source,
            &guest_path(&guest, &root, &source)?,
            &mut created,
        );
    }

    let guest_cwd = guest_path(&guest, &root, &cwd)?;
    let mut child = Command::new(bwrap);
    child.args(args).arg("--chdir").arg(guest_cwd);
    let safe_env = safe_environment(&runtime.environment);
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
) -> Result<Output, String> {
    let root = package_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let cwd = checked_working_directory(&root, working_directory)?;
    let mut allowed = Vec::new();
    for path in read {
        allowed.push((checked_path(&root, path)?, false));
    }
    for path in write {
        allowed.push((checked_path(&root, path)?, true));
    }
    for path in &runtime.roots {
        allowed.push((path.clone(), false));
    }
    let rules = landlock_rules(&allowed)?;
    let ruleset = create_landlock_ruleset(&rules).map_err(|error| error.to_string())?;
    let mut child = command(argv, &cwd, &safe_environment(&runtime.environment));
    unsafe {
        child.pre_exec(move || install_landlock(&ruleset));
    }
    collect_output(&mut child, input)
}

pub(super) fn checked_working_directory(root: &Path, working_directory: &Path) -> Result<PathBuf, String> {
    let cwd = working_directory
        .canonicalize()
        .map_err(|error| format!("command working directory is unavailable: {error}"))?;
    if !cwd.starts_with(root) || !cwd.is_dir() {
        return Err("command working directory must be inside the staged contract tree".into());
    }
    Ok(cwd)
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(format!("invalid staged path: {relative}"));
    }
    let full = root.join(path);
    let resolved = full
        .canonicalize()
        .map_err(|error| format!("{}: {error}", full.display()))?;
    if !resolved.starts_with(root) {
        return Err(format!("staged path escapes package root: {relative}"));
    }
    Ok(resolved)
}

fn guest_root(package_root: &Path) -> Result<String, String> {
    let name = package_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "staged package has no usable randomized root name".to_string())?;
    Ok(format!("/contract/{name}"))
}

fn guest_path(guest: &str, root: &Path, source: &Path) -> Result<PathBuf, String> {
    let relative = source
        .strip_prefix(root)
        .map_err(|error| error.to_string())?;
    Ok(Path::new(guest).join(relative))
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
    find_executable_in_path(name, std::env::var_os("PATH").as_deref().and_then(|path| path.to_str()))
}

fn find_executable_in_path(name: &str, search_path: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.exists().then(|| path.to_path_buf());
    }
    std::env::split_paths(&std::ffi::OsStr::new(search_path?))
        .map(|dir| dir.join(path))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
fn runtime_command_roots(argv: &[String]) -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").map(|value| value.to_string_lossy().into_owned());
    find_executable_in_path(&argv[0], path.as_deref())
        .map(|executable| runtime_command_roots_for_executable(&executable))
        .unwrap_or_default()
}

fn runtime_command_roots_for_executable(executable: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(directory) = executable.parent() {
        roots.push(directory.to_path_buf());
    }
    if let Ok(resolved) = executable.canonicalize() {
        if let Some(directory) = resolved.parent() {
            roots.push(directory.to_path_buf());
        }
    }
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

#[cfg(target_os = "linux")]
type LandlockRules = Vec<(OwnedFd, u64)>;

#[cfg(target_os = "linux")]
fn landlock_rules(allowed: &[(PathBuf, bool)]) -> Result<LandlockRules, String> {
    landlock_rules_with_open(allowed, open_landlock_rule_path)
}

#[cfg(target_os = "linux")]
fn landlock_rules_with_open(
    allowed: &[(PathBuf, bool)],
    mut open_rule_path: impl FnMut(&Path) -> io::Result<OwnedFd>,
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
fn open_landlock_rule_path(path: &Path) -> io::Result<OwnedFd> {
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
struct RulesetAttr {
    handled_access_fs: u64,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

#[cfg(target_os = "linux")]
enum LandlockCall<'a> {
    QueryAbi,
    CreateRuleset(&'a RulesetAttr),
    AddPathRule(i32, &'a PathBeneathAttr),
}

#[cfg(target_os = "linux")]
fn create_landlock_ruleset(rules: &LandlockRules) -> io::Result<OwnedFd> {
    const CREATE: libc::c_long = 444;
    const ADD: libc::c_long = 445;
    const VERSION: libc::c_int = 1;
    create_landlock_ruleset_with(rules, |call| match call {
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
fn create_landlock_ruleset_with(
    rules: &LandlockRules,
    mut syscall: impl FnMut(LandlockCall<'_>) -> libc::c_long,
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
fn install_landlock(ruleset: &OwnedFd) -> io::Result<()> {
    const RESTRICT: libc::c_long = 446;
    install_landlock_with(
        ruleset,
        || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) },
        |fd| unsafe { libc::syscall(RESTRICT, fd, 0) },
    )
}

#[cfg(target_os = "linux")]
fn install_landlock_with(
    ruleset: &OwnedFd,
    mut set_no_new_privs: impl FnMut() -> libc::c_int,
    mut restrict: impl FnMut(i32) -> libc::c_long,
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
type LandlockRules = Vec<(OwnedFd, u64)>;

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

#[cfg(all(test, target_os = "linux"))]
#[path = "../tests/backend_supplemental.rs"]
mod backend_supplemental_tests;
