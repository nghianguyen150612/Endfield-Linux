use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;

use endfield_unix_ipc::{LocalSocketServer, RuntimePaths, SOCKET_MODE, peer_uid_is_authorized};

#[test]
fn uid_policy_requires_exact_match_and_does_not_add_root_as_an_exception() {
    assert!(peer_uid_is_authorized(1000, 1000));
    assert!(!peer_uid_is_authorized(1000, 0));
    assert!(!peer_uid_is_authorized(1000, 1001));
}

#[test]
fn socket_namespace_and_socket_permissions_are_private() {
    let runtime = secure_runtime_directory();
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("validated runtime");
    let server = LocalSocketServer::bind_at(paths.clone()).expect("bind server");
    let directory_mode = fs::symlink_metadata(paths.application_directory())
        .expect("app directory metadata")
        .mode()
        & 0o777;
    let socket_metadata = fs::symlink_metadata(paths.socket_path()).expect("socket metadata");
    assert_eq!(directory_mode, 0o700);
    assert_eq!(socket_metadata.mode() & 0o777, SOCKET_MODE);
    assert_eq!(
        socket_metadata.uid(),
        nix::unistd::Uid::effective().as_raw()
    );
    drop(server);
    assert!(!paths.socket_path().exists());
}

#[test]
fn a_second_instance_cannot_replace_the_active_socket() {
    let runtime = secure_runtime_directory();
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("validated runtime");
    let server = LocalSocketServer::bind_at(paths.clone()).expect("first instance binds");
    let before = fs::symlink_metadata(paths.socket_path()).expect("first socket");
    assert!(LocalSocketServer::bind_at(paths.clone()).is_err());
    let after = fs::symlink_metadata(paths.socket_path()).expect("first socket remains");
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    drop(server);
}

#[test]
fn regular_files_and_symlinks_at_socket_path_fail_closed() {
    let runtime = secure_runtime_directory();
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("validated runtime");
    paths
        .prepare_server_directory()
        .expect("private app directory");
    let target = runtime.path().join("unrelated");
    fs::write(&target, b"preserve me").expect("unrelated file");
    std::os::unix::fs::symlink(&target, paths.socket_path()).expect("socket path symlink");
    assert!(LocalSocketServer::bind_at(paths.clone()).is_err());
    assert_eq!(
        fs::read(target).expect("unrelated file survives"),
        b"preserve me"
    );
}

#[test]
fn stale_socket_recovery_requires_a_same_uid_socket_with_no_live_kernel_entry() {
    let runtime = secure_runtime_directory();
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("validated runtime");
    paths
        .prepare_server_directory()
        .expect("private app directory");
    {
        let stale = UnixListener::bind(paths.socket_path()).expect("make stale socket");
        drop(stale);
    }
    let server = LocalSocketServer::bind_at(paths.clone()).expect("recover verified stale socket");
    assert!(paths.socket_path().exists());
    drop(server);
    assert!(!paths.socket_path().exists());
}

#[test]
fn runtime_directory_symlink_is_rejected() {
    let runtime = secure_runtime_directory();
    let alias = runtime.path().join("alias");
    std::os::unix::fs::symlink(runtime.path(), &alias).expect("runtime alias");
    assert!(RuntimePaths::for_runtime_directory(alias).is_err());
}

#[test]
fn client_refuses_an_unsafe_application_directory_or_socket_destination() {
    let runtime = secure_runtime_directory();
    let other = tempfile::tempdir().expect("symlink target");
    let paths = RuntimePaths::for_runtime_directory(runtime.path()).expect("validated runtime");
    std::os::unix::fs::symlink(other.path(), paths.application_directory())
        .expect("application directory symlink");
    assert!(paths.validate_client_socket().is_err());

    fs::remove_file(paths.application_directory()).expect("remove symlink");
    paths
        .prepare_server_directory()
        .expect("create app directory");
    let target = other.path().join("target.sock");
    let _listener = UnixListener::bind(&target).expect("socket target");
    std::os::unix::fs::symlink(target, paths.socket_path()).expect("socket symlink");
    assert!(paths.validate_client_socket().is_err());
}

fn secure_runtime_directory() -> tempfile::TempDir {
    let runtime = tempfile::tempdir().expect("runtime directory");
    fs::set_permissions(runtime.path(), fs::Permissions::from_mode(0o700))
        .expect("restrict runtime directory permissions");
    runtime
}
