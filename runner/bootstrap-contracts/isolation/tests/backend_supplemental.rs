use super::{
    create_landlock_ruleset, create_landlock_ruleset_with, install_landlock, install_landlock_with,
    landlock_rules, landlock_rules_with_open, runtime_command_roots, LandlockCall,
};
use std::fs;
use std::io;
use std::path::PathBuf;

fn fixture_fd() -> libc::c_long {
    let path = std::ffi::CString::new("/dev/null").unwrap_or_default();
    unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) as libc::c_long }
}

#[test]
// Dataset cases cannot inject an OS-level failure while opening a Landlock
// rule path. This checks that both the injected open error and a missing path
// are returned as actionable errors.
fn landlock_rule_path_open_errors_are_reported() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let error = landlock_rules_with_open(&[(directory.path().to_path_buf(), false)], |_| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture denied",
        ))
    })
    .expect_err("injected path-open failure must propagate");
    assert!(error.contains("cannot open Landlock rule path"));
    assert!(error.contains("fixture denied"));

    let missing = directory.path().join("does-not-exist");
    assert!(super::open_landlock_rule_path(&missing).is_err());
}

#[test]
// Dataset inputs cannot force individual Landlock syscalls to fail
// deterministically. This checks error propagation from ABI query, ruleset
// creation, and adding a path rule.
fn landlock_ruleset_reports_abi_creation_and_path_rule_failures() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let rules = landlock_rules(&[(directory.path().to_path_buf(), false)])
        .expect("real rule paths should be openable");

    let abi_error = create_landlock_ruleset_with(&rules, |call| match call {
        LandlockCall::QueryAbi => 0,
        _ => 0,
    });
    assert!(abi_error.is_err());

    let create_error = create_landlock_ruleset_with(&rules, |call| match call {
        LandlockCall::QueryAbi => 3,
        LandlockCall::CreateRuleset(_) => -1,
        LandlockCall::AddPathRule(_, _) => 0,
    });
    assert!(create_error.is_err());

    let add_error = create_landlock_ruleset_with(&rules, |call| match call {
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
    let directory = tempfile::tempdir().expect("temporary directory");
    let rules = landlock_rules(&[(directory.path().to_path_buf(), false)])
        .expect("real rule paths should be openable");
    let ruleset = create_landlock_ruleset_with(&rules, |call| match call {
        LandlockCall::QueryAbi => 3,
        LandlockCall::CreateRuleset(_) => fixture_fd(),
        LandlockCall::AddPathRule(_, _) => 0,
    })
    .expect("injected kernel calls should create a ruleset");
    assert!(install_landlock_with(&ruleset, || 0, |_| 0).is_ok());
    assert!(install_landlock_with(&ruleset, || -1, |_| 0).is_err());
    assert!(install_landlock_with(&ruleset, || 0, |_| -1).is_err());

    let real_ruleset = create_landlock_ruleset(&rules).expect("create real ruleset");
    let result = std::thread::spawn(move || install_landlock(&real_ruleset))
        .join()
        .expect("Landlock probe thread should exit");
    if let Err(error) = result {
        assert!(matches!(
            error.kind(),
            io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
        ));
    }
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

    let roots = runtime_command_roots(&[executable.to_string_lossy().into_owned()]);
    assert!(roots.contains(&PathBuf::from(directory.path())));
}
