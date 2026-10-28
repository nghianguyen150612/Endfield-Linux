use endfield_unix_runtime_linux::umu_discovery::{
    UmuAvailability, UmuDiscoveryEnvironment, discover_umu,
};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("endfield-umu-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
fn executable(path: &std::path::Path) {
    fs::write(path, b"synthetic executable fixture; never executed").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn absence_and_user_local_candidate_are_reported_without_host_paths() {
    let home = temp("home");
    let empty = UmuDiscoveryEnvironment::isolated(Some(home.clone()), None);
    assert_eq!(
        discover_umu(&empty, None).availability(),
        UmuAvailability::NotInstalled
    );
    assert_eq!(
        discover_umu(&empty, None).outcome(),
        endfield_unix_runtime_linux::RootOutcome::NotPresent
    );
    let path = home.join(".local/bin/umu-run");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    executable(&path);
    let report = discover_umu(&empty, None);
    assert_eq!(report.validated().count(), 1);
    assert_eq!(
        report.outcome(),
        endfield_unix_runtime_linux::RootOutcome::Inspected
    );
    assert_eq!(
        report.availability(),
        UmuAvailability::StructurallyValidated
    );
    assert!(report.candidates[0].version().is_none());
    assert_eq!(
        report.candidates[0].version_status(),
        endfield_unix_runtime_linux::umu_discovery::UmuVersionStatus::Unknown
    );
    assert!(!format!("{report:?}").contains(&home.display().to_string()));
    let _ = fs::remove_dir_all(home);
}

#[test]
fn path_hints_are_absolute_deduplicated_and_symlinks_are_bounded() {
    let root = temp("path");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let target = bin.join("umu-run-real");
    executable(&target);
    symlink(&target, bin.join("umu-run")).unwrap();
    let value = std::env::join_paths([
        bin.clone(),
        PathBuf::from("relative"),
        PathBuf::new(),
        bin.clone(),
    ])
    .unwrap();
    let env = UmuDiscoveryEnvironment::isolated(None, Some(value));
    let report = discover_umu(&env, None);
    assert_eq!(report.candidates.len(), 1);
    assert!(report.candidates[0].is_structurally_valid());
    assert!(report.candidates[0].revalidate());
    let revalidated = report.candidates[0].clone();
    let resolved = revalidated.resolved_path().unwrap().to_path_buf();
    let replacement = bin.join("replacement");
    executable(&replacement);
    fs::remove_file(&resolved).unwrap();
    symlink(&replacement, &resolved).unwrap();
    assert!(!revalidated.revalidate());
    let cycle = root.join("cycle");
    symlink("cycle-b", &cycle).unwrap();
    symlink("cycle", root.join("cycle-b")).unwrap();
    assert_eq!(
        discover_umu(&env, Some(&cycle)).availability(),
        UmuAvailability::InvalidExecutable
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_relative_or_traversing_paths_are_invalid_not_absent() {
    let root = temp("explicit-path");
    for path in [PathBuf::from("umu-run"), root.join("../umu-run")] {
        let report = discover_umu(&UmuDiscoveryEnvironment::isolated(None, None), Some(&path));
        assert_eq!(report.availability(), UmuAvailability::InvalidExecutable);
        assert_eq!(report.issues, ["invalid-explicit-path"]);
        assert_eq!(
            report.outcome(),
            endfield_unix_runtime_linux::RootOutcome::Failed
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn oversized_path_hint_is_truncation_not_false_absence() {
    let value = std::ffi::OsString::from(
        "x".repeat(endfield_unix_runtime_linux::umu_discovery::MAX_UMU_PATH_BYTES + 1),
    );
    let report = discover_umu(&UmuDiscoveryEnvironment::isolated(None, Some(value)), None);
    assert_eq!(
        report.outcome(),
        endfield_unix_runtime_linux::RootOutcome::Truncated
    );
    assert_eq!(report.availability(), UmuAvailability::IncompleteMetadata);
}

#[test]
fn non_executable_directory_and_special_file_are_rejected() {
    let root = temp("types");
    let no_exec = root.join("no-exec");
    fs::write(&no_exec, b"fixture").unwrap();
    fs::set_permissions(&no_exec, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        discover_umu(
            &UmuDiscoveryEnvironment::isolated(None, None),
            Some(&no_exec)
        )
        .availability(),
        UmuAvailability::InvalidExecutable
    );
    assert_eq!(
        discover_umu(&UmuDiscoveryEnvironment::isolated(None, None), Some(&root)).availability(),
        UmuAvailability::InvalidExecutable
    );
    let fifo = root.join("fifo");
    assert_eq!(
        unsafe {
            libc::mkfifo(
                std::ffi::CString::new(fifo.to_string_lossy().as_bytes())
                    .unwrap()
                    .as_ptr(),
                0o600,
            )
        },
        0
    );
    assert_eq!(
        discover_umu(&UmuDiscoveryEnvironment::isolated(None, None), Some(&fifo)).availability(),
        UmuAvailability::InvalidExecutable
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unreadable_candidate_parent_is_reported_as_permission_denied_when_enforced() {
    let root = temp("permission");
    let parent = root.join("blocked");
    fs::create_dir_all(&parent).unwrap();
    let candidate = parent.join("umu-run");
    executable(&candidate);
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).unwrap();
    let enforced = fs::symlink_metadata(&candidate)
        .err()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied);
    let report = discover_umu(
        &UmuDiscoveryEnvironment::isolated(None, None),
        Some(&candidate),
    );
    if enforced {
        assert_eq!(report.availability(), UmuAvailability::PermissionDenied);
    }
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn executable_change_invalidates_inspected_identity() {
    let root = temp("changed");
    let path = root.join("umu-run");
    executable(&path);
    let env = UmuDiscoveryEnvironment::isolated(None, None);
    let candidate = discover_umu(&env, Some(&path)).candidates.remove(0);
    fs::write(&path, b"replacement with different identity").unwrap();
    assert!(!candidate.revalidate());
    let _ = fs::remove_dir_all(root);
}
