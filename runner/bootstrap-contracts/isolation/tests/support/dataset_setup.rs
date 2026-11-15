#[derive(Clone, Copy)]
enum CaseSetup {
    UnusableTemporaryDirectory,
    HideBubblewrapFromPath,
    MockBubblewrapLauncher,
    PrintBubblewrapArguments,
    MissingLanguageRuntimeManifest,
    CreateUndeclaredLanguageResource,
    CreateUndeclaredEntrypoint,
    CreateLanguageSymlinkEscape,
    InvalidLanguagePackageMapping,
    UnmatchedLanguagePackageMapping,
    MissingMappedLanguagePackage,
    SetEnvironmentValue,
    ClearEnvironmentValue,
    InvalidUtf8EnvironmentValue,
    SetEmptyEnvironmentValue,
    ClearLangEnvironmentValue,
    LandlockInstallationProbe,
    BackendErrorPaths,
    StagingCopyErrorPaths,
    RemoveStagedRoot,
    RemoveStagedSupport,
    RemoveStagedWriteFile,
    BubblewrapPackageRoot,
    LandlockOpenRulePathFailure,
    LandlockAbiQueryFailure,
    LandlockRulesetCreateFailure,
    LandlockAddPathRuleFailure,
    LandlockSetNoNewPrivilegesFailure,
    LandlockRestrictSelfFailure,
}

const CASE_SETUP_BY_ID: &[(&str, CaseSetup)] = &[
    (
        "staged-tree-rejects-unusable-temporary-directory",
        CaseSetup::UnusableTemporaryDirectory,
    ),
    ("bubblewrap-unavailable", CaseSetup::HideBubblewrapFromPath),
    (
        "bubblewrap-missing-from-path",
        CaseSetup::HideBubblewrapFromPath,
    ),
    (
        "bubblewrap-command-stdin-and-stdout",
        CaseSetup::MockBubblewrapLauncher,
    ),
    (
        "bubblewrap-command-returns-nonzero-status",
        CaseSetup::MockBubblewrapLauncher,
    ),
    (
        "bubblewrap-command-maps-package-root",
        CaseSetup::BubblewrapPackageRoot,
    ),
    (
        "adapter-runtime-mounts-are-limited",
        CaseSetup::PrintBubblewrapArguments,
    ),
    (
        "language-package-runtime-manifest-missing",
        CaseSetup::MissingLanguageRuntimeManifest,
    ),
    (
        "language-package-environment-path-outside-files",
        CaseSetup::CreateUndeclaredLanguageResource,
    ),
    (
        "language-package-entrypoint-not-declared",
        CaseSetup::CreateUndeclaredEntrypoint,
    ),
    (
        "language-package-rejects-symlink-escape",
        CaseSetup::CreateLanguageSymlinkEscape,
    ),
    (
        "language-package-invalid-environment-mapping",
        CaseSetup::InvalidLanguagePackageMapping,
    ),
    (
        "language-package-rejects-invalid-runner-configuration",
        CaseSetup::InvalidLanguagePackageMapping,
    ),
    (
        "language-package-unmatched-mapping-falls-back-to-repository",
        CaseSetup::UnmatchedLanguagePackageMapping,
    ),
    (
        "language-package-mapped-directory-missing",
        CaseSetup::MissingMappedLanguagePackage,
    ),
    (
        "command-expands-environment-reference",
        CaseSetup::SetEnvironmentValue,
    ),
    (
        "command-rejects-unset-environment-reference",
        CaseSetup::ClearEnvironmentValue,
    ),
    (
        "command-rejects-invalid-utf8-environment-reference",
        CaseSetup::InvalidUtf8EnvironmentValue,
    ),
    (
        "landlock-uses-default-language-environment",
        CaseSetup::ClearLangEnvironmentValue,
    ),
    (
        "landlock-installation-failure-scenarios",
        CaseSetup::LandlockInstallationProbe,
    ),
    ("command-backend-error-paths", CaseSetup::BackendErrorPaths),
    ("staging-copy-error-paths", CaseSetup::StagingCopyErrorPaths),
    (
        "command-rejects-empty-expanded-executable",
        CaseSetup::SetEmptyEnvironmentValue,
    ),
    (
        "command-none-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-bwrap-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-landlock-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "command-bwrap-reports-missing-staged-support",
        CaseSetup::RemoveStagedSupport,
    ),
    (
        "command-landlock-reports-missing-staged-support",
        CaseSetup::RemoveStagedSupport,
    ),
    (
        "command-bwrap-reports-missing-staged-write-file",
        CaseSetup::RemoveStagedWriteFile,
    ),
    (
        "command-landlock-reports-missing-staged-write-file",
        CaseSetup::RemoveStagedWriteFile,
    ),
    (
        "command-at-reports-missing-root",
        CaseSetup::RemoveStagedRoot,
    ),
    (
        "landlock-open-rule-path-failure",
        CaseSetup::LandlockOpenRulePathFailure,
    ),
    (
        "landlock-abi-query-failure",
        CaseSetup::LandlockAbiQueryFailure,
    ),
    (
        "landlock-ruleset-create-failure",
        CaseSetup::LandlockRulesetCreateFailure,
    ),
    (
        "landlock-add-path-rule-failure",
        CaseSetup::LandlockAddPathRuleFailure,
    ),
    (
        "landlock-set-no-new-privileges-failure",
        CaseSetup::LandlockSetNoNewPrivilegesFailure,
    ),
    (
        "landlock-restrict-self-failure",
        CaseSetup::LandlockRestrictSelfFailure,
    ),
];

pub(super) fn run_staging_failure(
    case_id: &str,
    setup_enabled: bool,
    plan: &runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: &serde_json::Value,
) -> Result<(), String> {
    let mut plan = plan.clone();
    let temporary = if setup_enabled {
        if case_id != "staged-tree-rejects-unusable-temporary-directory" {
            return Err(format!(
                "dataset case {case_id:?} has unexpected setup: true"
            ));
        }
        let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let not_a_directory = temporary.path().join("not-a-directory");
        std::fs::write(&not_a_directory, "file").map_err(|error| error.to_string())?;
        let _temporary_directory =
            EnvironmentGuard::set("TMPDIR", not_a_directory.into_os_string());
        let result = run_staging_case(&mut plan, input, expected);
        drop(_temporary_directory);
        return result;
    } else {
        if case_id == "staged-tree-rejects-unusable-temporary-directory" {
            return Err(format!("dataset case {case_id:?} must declare setup: true"));
        }
        None::<tempfile::TempDir>
    };
    let _temporary = temporary;
    run_staging_case(&mut plan, input, expected)
}

fn run_staging_case(
    plan: &mut runner_manifest::ManifestPlan,
    input: &serde_json::Value,
    expected: &serde_json::Value,
) -> Result<(), String> {
    apply_staging_manifest_input(plan, input)?;
    match runner_isolation::StagedTree::create(plan) {
        Ok(tree) => {
            if expected["created"] != true {
                return Err("staging succeeded but expected.error_contains was declared".into());
            }
            let writes = &input["manifest"]["filesystem"]["write"];
            if let Some(writes) = writes.as_array() {
                for path in writes {
                    let path = path
                        .as_str()
                        .ok_or_else(|| "filesystem.write items must be strings".to_string())?;
                    if !tree.root.join(path).exists() {
                        return Err(format!("declared writable path {path:?} was not staged"));
                    }
                }
            }
            Ok(())
        }
        Err(error) => {
            let needle = expected["error_contains"]
                .as_str()
                .ok_or_else(|| "expected.error_contains must be a string".to_string())?;
            if error.contains(needle) {
                Ok(())
            } else {
                Err(format!("unexpected staging error: {error}"))
            }
        }
    }
}

fn apply_staging_manifest_input(
    plan: &mut runner_manifest::ManifestPlan,
    input: &serde_json::Value,
) -> Result<(), String> {
    if input["operation"] != "stage-contract-tree" {
        return Err("input.operation must be stage-contract-tree".into());
    }
    let manifest = input["manifest"]
        .as_object()
        .ok_or_else(|| "input.manifest must be an object".to_string())?;
    let mut has_manifest_input = false;
    if let Some(filesystem) = manifest.get("filesystem") {
        let filesystem = filesystem
            .as_object()
            .ok_or_else(|| "input.manifest.filesystem must be an object".to_string())?;
        reject_unknown_keys(filesystem, &["read", "write"], "input.manifest.filesystem")?;
        if let Some(read) = filesystem.get("read") {
            plan.manifest.filesystem.read = string_list(read, "input.manifest.filesystem.read")?;
            has_manifest_input = true;
        }
        if let Some(write) = filesystem.get("write") {
            plan.manifest.filesystem.write = string_list(write, "input.manifest.filesystem.write")?;
            has_manifest_input = true;
        }
    }
    if let Some(files) = manifest.get("files") {
        let files = files
            .as_object()
            .ok_or_else(|| "input.manifest.files must be an object".to_string())?;
        reject_unknown_keys(files, &["nested_contracts"], "input.manifest.files")?;
        if let Some(nested_contracts) = files.get("nested_contracts") {
            plan.manifest.files.nested_contracts =
                string_list(nested_contracts, "input.manifest.files.nested_contracts")?;
            has_manifest_input = true;
        }
    }
    reject_unknown_keys(manifest, &["filesystem", "files"], "input.manifest")?;
    reject_unknown_keys(
        input.as_object().expect("validated operation object"),
        &["operation", "manifest"],
        "input",
    )?;
    if !has_manifest_input {
        return Err("input.manifest must specify filesystem.read, filesystem.write, or files.nested_contracts".into());
    }
    Ok(())
}

fn reject_unknown_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
    field: &str,
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{field} contains unsupported field {key:?}"));
    }
    Ok(())
}

fn string_list(value: &serde_json::Value, field: &str) -> Result<Vec<String>, String> {
    let values = value
        .as_array()
        .ok_or_else(|| format!("{field} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{field} items must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(format!("{field} must contain at least one path"));
    }
    Ok(values)
}

pub(super) fn run_with_case_setup<T: Send>(
    case_id: &str,
    setup_enabled: bool,
    package_root: &std::path::Path,
    run: impl FnOnce() -> Result<T, String> + Send,
) -> Result<T, String> {
    let setup = CASE_SETUP_BY_ID
        .iter()
        .find_map(|(id, setup)| (*id == case_id).then_some(*setup));
    match (setup_enabled, setup) {
        (false, Some(_)) => Err(format!(
            "dataset case {case_id:?} has test setup but does not declare setup: true"
        )),
        (true, None) => Err(format!(
            "dataset case {case_id:?} declares setup: true but has no test setup"
        )),
        (true, Some(CaseSetup::UnusableTemporaryDirectory)) => Err(format!(
            "dataset case {case_id:?} setup must run before staging"
        )),
        (true, Some(CaseSetup::HideBubblewrapFromPath)) => {
            let _path = EnvironmentGuard::clear("PATH");
            run()
        }
        (true, Some(CaseSetup::MockBubblewrapLauncher)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then shift; exec \"$@\"; fi\n  shift\ndone\nexit 125\n", run)
        }
        (true, Some(CaseSetup::BubblewrapPackageRoot)) => {
            let _root = EnvironmentGuard::set(
                "OPEN_CONTRACT_TEST_PACKAGE_ROOT",
                package_root.as_os_str().to_owned(),
            );
            run_with_bubblewrap_launcher(
                "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then shift; exec \"$@\"; fi\n  shift\ndone\nexit 125\n",
                run,
            )
        }
        (true, Some(CaseSetup::PrintBubblewrapArguments)) => {
            run_with_bubblewrap_launcher("#!/bin/sh\nprintf '%s\\n' \"$@\"\n", run)
        }
        (true, Some(CaseSetup::MissingLanguageRuntimeManifest)) => {
            let _setting = EnvironmentGuard::set(
                "OPEN_CONTRACT_ISOLATION_OMIT_LANGUAGE_RUNTIME_MANIFEST",
                "1",
            );
            run()
        }
        (true, Some(CaseSetup::CreateUndeclaredLanguageResource)) => {
            let _setting = EnvironmentGuard::set(
                "OPEN_CONTRACT_ISOLATION_CREATE_UNDECLARED_LANGUAGE_RESOURCE",
                "1",
            );
            run()
        }
        (true, Some(CaseSetup::CreateUndeclaredEntrypoint)) => {
            let _setting = EnvironmentGuard::set(
                "OPEN_CONTRACT_ISOLATION_CREATE_UNDECLARED_LANGUAGE_ENTRYPOINT",
                "1",
            );
            run()
        }
        (true, Some(CaseSetup::CreateLanguageSymlinkEscape)) => run(),
        (true, Some(CaseSetup::InvalidLanguagePackageMapping)) => {
            let _mapping = EnvironmentGuard::set(
                "OPEN_CONTRACT_LANGUAGE_PACKAGES",
                "{" ,
            );
            run()
        }
        (true, Some(CaseSetup::UnmatchedLanguagePackageMapping)) => {
            let _mapping = EnvironmentGuard::set(
                "OPEN_CONTRACT_LANGUAGE_PACKAGES",
                r#"{"another-language":"/tmp"}"#,
            );
            run()
        }
        (true, Some(CaseSetup::MissingMappedLanguagePackage)) => {
            let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
            let mapping = serde_json::json!({
                "missing-package": temp.path().join("package-does-not-exist")
            });
            let _mapping = EnvironmentGuard::set(
                "OPEN_CONTRACT_LANGUAGE_PACKAGES",
                mapping.to_string(),
            );
            run()
        }
        (true, Some(CaseSetup::SetEnvironmentValue)) => {
            let _value = EnvironmentGuard::set("OPEN_CONTRACT_TEST_VALUE", "contract supplied value");
            run()
        }
        (true, Some(CaseSetup::ClearEnvironmentValue)) => {
            let _value = EnvironmentGuard::clear("OPEN_CONTRACT_TEST_UNSET");
            run()
        }
        (true, Some(CaseSetup::InvalidUtf8EnvironmentValue)) => {
            use std::os::unix::ffi::OsStringExt;
            let _value = EnvironmentGuard::set(
                "OPEN_CONTRACT_TEST_INVALID_UTF8",
                std::ffi::OsString::from_vec(vec![0xff]),
            );
            run()
        }
        (true, Some(CaseSetup::SetEmptyEnvironmentValue)) => {
            let _value = EnvironmentGuard::set("OPEN_CONTRACT_TEST_EMPTY", "   ");
            run()
        }
        (true, Some(CaseSetup::LandlockInstallationProbe)) => {
            crate::test_fixture::exercise_landlock_installation_paths()?;
            run()
        }
        (true, Some(CaseSetup::BackendErrorPaths)) => {
            crate::test_fixture::exercise_backend_error_paths()?;
            run()
        }
        (true, Some(CaseSetup::StagingCopyErrorPaths)) => {
            crate::test_fixture::exercise_staging_copy_error_paths()?;
            run()
        }
        (true, Some(CaseSetup::ClearLangEnvironmentValue)) => {
            let _lang = EnvironmentGuard::clear("LANG");
            run()
        }
        (true, Some(CaseSetup::RemoveStagedRoot)) => {
            std::fs::remove_dir_all(package_root).map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::RemoveStagedSupport)) => {
            std::fs::remove_dir_all(package_root.join("support"))
                .map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::RemoveStagedWriteFile)) => {
            std::fs::remove_file(package_root.join("reports/result.json"))
                .map_err(|error| error.to_string())?;
            run()
        }
        (true, Some(CaseSetup::LandlockOpenRulePathFailure)) => {
            with_landlock_filter(LandlockFilter::OpenRulePath, run)
        }
        (true, Some(CaseSetup::LandlockAbiQueryFailure)) => {
            with_landlock_filter(LandlockFilter::AbiQuery, run)
        }
        (true, Some(CaseSetup::LandlockRulesetCreateFailure)) => {
            with_landlock_filter(LandlockFilter::CreateRuleset, run)
        }
        (true, Some(CaseSetup::LandlockAddPathRuleFailure)) => {
            with_landlock_filter(LandlockFilter::AddPathRule, run)
        }
        (true, Some(CaseSetup::LandlockSetNoNewPrivilegesFailure)) => {
            with_landlock_filter(LandlockFilter::SetNoNewPrivileges, run)
        }
        (true, Some(CaseSetup::LandlockRestrictSelfFailure)) => {
            with_landlock_filter(LandlockFilter::RestrictSelf, run)
        }
        (false, None) => run(),
    }
}

#[derive(Clone, Copy)]
enum LandlockFilter {
    OpenRulePath,
    AbiQuery,
    CreateRuleset,
    AddPathRule,
    SetNoNewPrivileges,
    RestrictSelf,
}

fn with_landlock_filter<T, F>(filter: LandlockFilter, run: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> Result<T, String> + Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                install_landlock_filter(filter)?;
                run()
            })
            .join()
            .map_err(|_| "Landlock setup thread panicked".to_string())?
    })
}

#[cfg(target_os = "linux")]
fn install_landlock_filter(filter: LandlockFilter) -> Result<(), String> {
    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    const BPF_LD_W_ABS: u16 = 0x20;
    const BPF_JMP_JEQ_K: u16 = 0x15;
    const BPF_RET_K: u16 = 0x06;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    const SYS_LANDLOCK_CREATE_RULESET: u32 = 444;
    const SYS_LANDLOCK_ADD_RULE: u32 = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: u32 = 446;
    const PR_SET_SECCOMP: libc::c_int = 22;
    const SECCOMP_MODE_FILTER: libc::c_int = 2;
    const O_PATH: u32 = 0x0020_0000;
    const O_CLOEXEC: u32 = 0x0008_0000;

    fn statement(code: u16, k: u32) -> SockFilter {
        SockFilter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }
    fn jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
        SockFilter { code, jt, jf, k }
    }

    let deny = SECCOMP_RET_ERRNO | libc::EPERM as u32;
    let allow = statement(BPF_RET_K, SECCOMP_RET_ALLOW);
    let deny_rule = statement(BPF_RET_K, deny);
    let instructions = match filter {
        LandlockFilter::OpenRulePath => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, libc::SYS_openat as u32, 0, 3),
            statement(BPF_LD_W_ABS, 32),
            jump(BPF_JMP_JEQ_K, O_PATH | O_CLOEXEC, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::AbiQuery => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_CREATE_RULESET, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::CreateRuleset => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_CREATE_RULESET, 0, 3),
            statement(BPF_LD_W_ABS, 16),
            jump(BPF_JMP_JEQ_K, 0, 1, 0),
            deny_rule,
            allow,
        ],
        LandlockFilter::AddPathRule => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_ADD_RULE, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::SetNoNewPrivileges => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, libc::SYS_prctl as u32, 0, 3),
            statement(BPF_LD_W_ABS, 16),
            jump(BPF_JMP_JEQ_K, libc::PR_SET_NO_NEW_PRIVS as u32, 0, 1),
            deny_rule,
            allow,
        ],
        LandlockFilter::RestrictSelf => vec![
            statement(BPF_LD_W_ABS, 0),
            jump(BPF_JMP_JEQ_K, SYS_LANDLOCK_RESTRICT_SELF, 0, 1),
            deny_rule,
            allow,
        ],
    };
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } < 0 {
        return Err(format!(
            "cannot enable seccomp setup: {}",
            std::io::Error::last_os_error()
        ));
    }
    let program = SockFprog {
        len: instructions.len() as u16,
        filter: instructions.as_ptr(),
    };
    let result = unsafe {
        libc::prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &program as *const SockFprog,
        )
    };
    if result < 0 {
        return Err(format!(
            "cannot install seccomp setup: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn install_landlock_filter(_filter: LandlockFilter) -> Result<(), String> {
    Err("Landlock dataset setup requires Linux seccomp".into())
}

fn run_with_bubblewrap_launcher<T>(
    content: &str,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    use std::os::unix::fs::PermissionsExt;
    let fake_bin = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = fake_bin.path().join("bwrap");
    std::fs::write(&launcher, content).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let path = match std::env::var_os("PATH") {
        Some(previous) => {
            let mut paths = vec![fake_bin.path().to_path_buf()];
            paths.extend(std::env::split_paths(&previous));
            std::env::join_paths(paths).map_err(|error| error.to_string())?
        }
        None => fake_bin.path().as_os_str().to_owned(),
    };
    let _path = EnvironmentGuard::set("PATH", path);
    run()
}

struct EnvironmentGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentGuard {
    fn set(name: &'static str, value: impl Into<std::ffi::OsString>) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value.into());
        Self { name, previous }
    }

    fn clear(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}
