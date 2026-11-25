use crate::backend::{
    collect_output, collect_output_with_wait, create_landlock_ruleset,
    create_landlock_ruleset_with, install_landlock, install_landlock_with, landlock_pre_exec,
    landlock_rules, landlock_rules_with_open, open_landlock_rule_path, run, runtime,
    runtime_command_roots_for_executable, LandlockCall,
};
use crate::Backend;
use std::fs;
use std::io;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn embedded_isolation_runtime_defaults_do_not_require_runner_checkout() {
    for backend in [Backend::Bubblewrap, Backend::Landlock] {
        runtime(backend, None)
            .unwrap_or_else(|error| panic!("{backend:?} embedded runtime failed: {error}"));
    }
}

#[test]
fn isolation_runtime_override_is_loaded_without_runner_checkout() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let readable = temporary
        .path()
        .canonicalize()
        .expect("canonical temp path");
    let manifest = temporary.path().join("override.yaml");
    fs::write(
        &manifest,
        format!(
            "version: 1\nread_only:\n  - {}\ncommand_executable: false\n",
            readable.display()
        ),
    )
    .expect("write override manifest");
    runtime(Backend::Bubblewrap, Some(&manifest))
        .expect("explicit override should not need the runner checkout");

    fs::write(&manifest, "version: 2\n").expect("write invalid override manifest");
    let error = match runtime(Backend::Bubblewrap, Some(&manifest)) {
        Ok(_) => panic!("unsupported override version must fail validation"),
        Err(error) => error,
    };
    assert!(error.contains("unsupported isolation runtime manifest version 2"));
}

#[test]
fn backends_without_runtime_manifests_do_not_require_runner_checkout() {
    for backend in [Backend::None, Backend::TempDir] {
        runtime(backend, None)
            .unwrap_or_else(|error| panic!("{backend:?} runtime unexpectedly failed: {error}"));
    }
}

fn fixture_fd() -> libc::c_long {
    let path = std::ffi::CString::new("/dev/null").unwrap_or_default();
    unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) as libc::c_long }
}

#[test]
fn contract_commands_get_unique_temporary_directories_cleaned_after_exit() {
    let root = tempfile::tempdir().expect("contract root");
    let report = root.path().join("temporary-environment.txt");
    let script = "printf '%s\\n%s\\n%s\\n' \"$TMPDIR\" \"$TMP\" \"$TEMP\" > \"$1\"; touch \"$TMPDIR/contract-marker\"";
    let command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        script.to_string(),
        "contract-command".to_string(),
        report.to_string_lossy().into_owned(),
    ];
    let mut supported = vec![Backend::None, Backend::TempDir];
    if crate::backend::probe_bubblewrap(None).is_ok() {
        supported.push(Backend::Bubblewrap);
    }
    if crate::backend::probe_landlock(None).is_ok() {
        supported.push(Backend::Landlock);
    }

    let mut observed_host_dirs = Vec::new();
    for backend in supported {
        let output = run(
            backend,
            root.path(),
            root.path(),
            &[],
            &[".".to_string()],
            &command,
            b"",
            None,
            false,
        )
        .unwrap_or_else(|error| panic!("{backend:?} command failed: {error}"));
        assert!(output.status.success(), "{backend:?} command failed");
        let paths = fs::read_to_string(&report).expect("temporary env report");
        let paths = paths.lines().collect::<Vec<_>>();
        assert_eq!(paths.len(), 3);
        assert!(paths.iter().all(|path| *path == paths[0]));
        if backend != Backend::Bubblewrap {
            assert!(PathBuf::from(paths[0]).starts_with(std::env::temp_dir()));
            assert!(
                !PathBuf::from(paths[0]).exists(),
                "{backend:?} tempdir remains after command exit"
            );
            observed_host_dirs.push(paths[0].to_owned());
        } else {
            assert_eq!(paths[0], "/tmp/open-contract-command-tmp");
        }
    }
    let host_dir_count = observed_host_dirs.len();
    observed_host_dirs.sort();
    observed_host_dirs.dedup();
    assert_eq!(
        observed_host_dirs.len(),
        host_dir_count,
        "each command gets a distinct tempdir"
    );
}

#[test]
fn command_temporary_directory_is_removed_after_nonzero_exit() {
    let root = tempfile::tempdir().expect("contract root");
    let report = root.path().join("temporary-environment.txt");
    let command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "printf '%s' \"$TMPDIR\" > \"$1\"; touch \"$TMPDIR/contract-marker\"; exit 13".to_string(),
        "contract-command".to_string(),
        report.to_string_lossy().into_owned(),
    ];
    let output = run(
        Backend::None,
        root.path(),
        root.path(),
        &[],
        &[],
        &command,
        b"",
        None,
        false,
    )
    .expect("nonzero status is still returned as command output");
    assert_eq!(output.status.code(), Some(13));
    let tmpdir = fs::read_to_string(report).expect("temporary path report");
    assert!(!PathBuf::from(tmpdir).exists());
}

#[test]
// Dataset cases cannot inject an OS-level failure while opening a Landlock
// rule path. This checks that both the injected open error and a missing path
// are returned as actionable errors.
fn landlock_rule_path_open_errors_are_reported() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let error = landlock_rules_with_open(&[(directory.path().to_path_buf(), false)], &mut |_| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture denied",
        ))
    })
    .expect_err("injected path-open failure must propagate");
    assert!(error.contains("cannot open Landlock rule path"));
    assert!(error.contains("fixture denied"));

    let missing = directory.path().join("does-not-exist");
    assert!(open_landlock_rule_path(&missing).is_err());
}

#[test]
// Dataset inputs cannot force individual Landlock syscalls to fail
// deterministically. This checks error propagation from ABI query, ruleset
// creation, and adding a path rule.
fn landlock_ruleset_reports_abi_creation_and_path_rule_failures() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let rules = landlock_rules(&[(directory.path().to_path_buf(), false)])
        .expect("real rule paths should be openable");

    let abi_error = create_landlock_ruleset_with(&rules, &mut |call| match call {
        LandlockCall::QueryAbi => 0,
        _ => 0,
    });
    assert!(abi_error.is_err());

    let create_error = create_landlock_ruleset_with(&rules, &mut |call| match call {
        LandlockCall::QueryAbi => 3,
        LandlockCall::CreateRuleset(_) => -1,
        LandlockCall::AddPathRule(_, _) => 0,
    });
    assert!(create_error.is_err());

    let add_error = create_landlock_ruleset_with(&rules, &mut |call| match call {
        LandlockCall::QueryAbi => 3,
        LandlockCall::CreateRuleset(_) => fixture_fd(),
        LandlockCall::AddPathRule(_, _) => -1,
    });
    assert!(add_error.is_err());
}

#[test]
// Dataset cases cannot reliably control kernel ABI support or syscall results.
// This checks successful ruleset installation plus failures from setting
// no_new_privs and restricting the calling thread.
fn landlock_ruleset_and_restriction_success_paths_are_handled() {
    exercise_landlock_installation_paths().expect("Landlock installation scenarios");
}

pub(crate) fn exercise_landlock_installation_paths() -> Result<(), String> {
    let directory = tempfile::tempdir().expect("temporary directory");
    let nul_path = std::path::Path::new("/invalid\0landlock-path");
    assert!(open_landlock_rule_path(nul_path).is_err());
    let missing = directory.path().join("missing-rule-path");
    assert!(landlock_rules(&[(missing, false)]).is_err());
    let duplicate_rules = landlock_rules_with_open(
        &[
            (directory.path().to_path_buf(), false),
            (directory.path().to_path_buf(), true),
        ],
        &mut |_| {
            let fd = fixture_fd();
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
        },
    )
    .map_err(|error| error.to_string())?;
    assert!(!duplicate_rules.is_empty());

    let rules = landlock_rules(&[(directory.path().to_path_buf(), false)])
        .expect("real rule paths should be openable");

    for abi in [1, 2] {
        let ruleset = create_landlock_ruleset_with(&rules, &mut |call| match call {
            LandlockCall::QueryAbi => abi,
            LandlockCall::CreateRuleset(_) => fixture_fd(),
            LandlockCall::AddPathRule(_, _) => 0,
        })
        .expect("supported Landlock ABI should create a ruleset");
        drop(ruleset);
    }

    assert!(runtime_command_roots_for_executable(std::path::Path::new("")).len() <= 1);
    let ruleset = create_landlock_ruleset_with(&rules, &mut |call| match call {
        LandlockCall::QueryAbi => 3,
        LandlockCall::CreateRuleset(_) => fixture_fd(),
        LandlockCall::AddPathRule(_, _) => 0,
    })
    .expect("injected kernel calls should create a ruleset");
    assert!(install_landlock_with(&ruleset, &mut || 0, &mut |_| 0).is_ok());
    assert!(install_landlock_with(&ruleset, &mut || -1, &mut |_| 0).is_err());
    assert!(install_landlock_with(&ruleset, &mut || 0, &mut |_| -1).is_err());

    let real_ruleset = create_landlock_ruleset(&rules).map_err(|error| error.to_string())?;
    let result = std::thread::spawn(move || install_landlock(&real_ruleset))
        .join()
        .map_err(|_| "Landlock probe thread panicked".to_string())?;
    if let Err(error) = result {
        assert!(matches!(
            error.kind(),
            io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
        ));
    }
    Ok(())
}

pub(crate) fn exercise_backend_error_paths() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let outside = tempfile::tempdir().map_err(|error| error.to_string())?;
    let command = vec!["/bin/true".to_string()];

    for backend in [Backend::None, Backend::Landlock] {
        let error = run(
            backend,
            root.path(),
            outside.path(),
            &[],
            &[],
            &command,
            b"",
            None,
            false,
        )
        .expect_err("outside command working directory must be rejected");
        assert!(error.contains("inside the staged contract tree"));
    }

    let launcher_dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let launcher = launcher_dir.path().join("bwrap");
    fs::write(&launcher, "#!/bin/sh\nexit 0\n").map_err(|error| error.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let previous_path = std::env::var_os("PATH");
    std::env::set_var("PATH", launcher_dir.path());
    let bubblewrap_error = run(
        Backend::Bubblewrap,
        root.path(),
        outside.path(),
        &[],
        &[],
        &command,
        b"",
        None,
        false,
    )
    .expect_err("outside Bubblewrap working directory must be rejected");
    if let Some(path) = previous_path {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    assert!(bubblewrap_error.contains("inside the staged contract tree"));

    let stdin_error = {
        let mut child = Command::new("/bin/true");
        child.stdin(Stdio::null());
        collect_output(&mut child, b"").expect_err("non-piped stdin must be rejected")
    };
    assert!(stdin_error.contains("command stdin unavailable"));

    let wait_error = {
        let mut child = Command::new("/bin/true");
        child.stdin(Stdio::piped());
        let mut wait = |mut child: std::process::Child| {
            let _ = child.wait();
            Err(std::io::Error::other("injected wait failure"))
        };
        collect_output_with_wait(&mut child, b"", &mut wait)
            .expect_err("injected wait failure must propagate")
    };
    assert!(wait_error.contains("injected wait failure"));

    runtime(Backend::None, None).expect("none backend does not use runtime manifests");

    let ruleset_fd = fixture_fd();
    if ruleset_fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let ruleset = unsafe { std::os::fd::OwnedFd::from_raw_fd(ruleset_fd as i32) };
    let mut pre_exec = landlock_pre_exec(ruleset, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected pre-exec installation failure",
        ))
    });
    assert!(pre_exec().is_err());
    Ok(())
}

#[test]
// Dataset cases do not control the host executable's location. This checks
// that an executable outside the standard system roots contributes its parent
// directory to the runtime access roots.
fn executable_paths_outside_system_roots_are_added() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let executable = directory.path().join("contract-command");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write command fixture");
    let mut permissions = fs::metadata(&executable)
        .expect("read command fixture metadata")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).expect("make fixture executable");

    let roots = runtime_command_roots_for_executable(&executable);
    assert!(roots.contains(&PathBuf::from(directory.path())));
}
