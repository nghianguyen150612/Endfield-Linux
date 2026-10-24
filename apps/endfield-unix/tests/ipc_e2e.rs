#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tempfile::TempDir;

const READY_LIMIT: Duration = Duration::from_secs(5);
const EXIT_LIMIT: Duration = Duration::from_secs(4);

struct ManagedDaemon {
    child: Child,
    runtime_dir: PathBuf,
}

impl ManagedDaemon {
    fn stop(&mut self, signal: Signal) {
        let pid = i32::try_from(self.child.id()).expect("child pid fits pid_t");
        kill(Pid::from_raw(pid), signal).expect("signal only the owned daemon child");
        let deadline = Instant::now() + EXIT_LIMIT;
        loop {
            if let Some(status) = self.child.try_wait().expect("check daemon child") {
                assert_eq!(status.code(), Some(0), "clean signal shutdown");
                assert!(!self.socket_path().exists(), "owned socket was removed");
                return;
            }
            if Instant::now() >= deadline {
                panic!("daemon did not exit within the bounded shutdown deadline");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn socket_path(&self) -> PathBuf {
        self.runtime_dir
            .join("endfield-unix")
            .join("endfield-unixd.sock")
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("check daemon child") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "daemon child did not exit in time"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ManagedDaemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn subprocess_daemon_and_cli_complete_ping_status_malformed_frame_and_sigterm() {
    let directory = tempfile::tempdir().expect("isolated test environment");
    let runtime_dir = make_runtime_dir(&directory);
    let daemon_binary = target_directory().join("endfield-unixd");
    assert!(daemon_binary.is_file(), "workspace daemon binary exists");
    let mut daemon = start_daemon(&daemon_binary, &directory, &runtime_dir);
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_endfield-unix"));

    wait_for_cli_ping(&cli, &runtime_dir);
    let ping = run_cli(&cli, &runtime_dir, &["service", "ping"]);
    assert_eq!(ping.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ping.stdout).contains("Pong"));

    let status = run_cli(&cli, &runtime_dir, &["service", "status"]);
    assert_eq!(status.status.code(), Some(0));
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(status.contains("Daemon: running"));
    assert!(status.contains("IPC: connected"));
    assert!(status.contains("Protocol: 1.0"));
    assert!(status.contains("Service: endfield-unixd"));
    assert!(status.contains("Implemented capabilities: local_ipc, ping, status"));
    assert!(status.contains("Not implemented: telemetry, game_session_supervision"));

    let mut second = start_daemon(&daemon_binary, &directory, &runtime_dir);
    assert_eq!(second.wait_for_exit(EXIT_LIMIT).code(), Some(70));
    assert!(
        daemon.socket_path().exists(),
        "second instance preserved active socket"
    );
    assert_eq!(
        run_cli(&cli, &runtime_dir, &["service", "ping"])
            .status
            .code(),
        Some(0)
    );

    thread::scope(|scope| {
        let clients = (0..8)
            .map(|_| scope.spawn(|| run_cli(&cli, &runtime_dir, &["service", "ping"])))
            .collect::<Vec<_>>();
        for client in clients {
            assert_eq!(
                client.join().expect("concurrent CLI process").status.code(),
                Some(0)
            );
        }
    });

    let client_capacity =
        endfield_unix_ipc::MAX_SIMULTANEOUS_CLIENTS + endfield_unix_ipc::MAX_QUEUED_CLIENTS;
    let mut slow_clients = Vec::with_capacity(client_capacity);
    for index in 0..client_capacity {
        let mut client = UnixStream::connect(daemon.socket_path())
            .expect("connect a bounded-concurrency test client");
        if index < endfield_unix_ipc::MAX_SIMULTANEOUS_CLIENTS {
            client
                .write_all(&[0, 0])
                .expect("hold a worker with an incomplete frame");
        }
        slow_clients.push(client);
    }
    thread::sleep(Duration::from_millis(50));
    let saturated = UnixStream::connect(daemon.socket_path())
        .expect("connect one client beyond the application capacity");
    saturated
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("bound saturation observation");
    let mut probe = [0u8; 1];
    assert_eq!(
        (&saturated)
            .read(&mut probe)
            .expect("over-capacity connection closes"),
        0,
        "the daemon closes clients beyond the four-worker, twelve-queue limit"
    );
    drop(saturated);
    drop(slow_clients);
    assert_eq!(
        run_cli(&cli, &runtime_dir, &["service", "ping"])
            .status
            .code(),
        Some(0),
        "the daemon recovers after over-capacity clients disconnect"
    );

    let mut raw = UnixStream::connect(daemon.socket_path()).expect("connect raw local client");
    let malformed = br#"{"protocol_major":1,"protocol_minor":0,"request_id":"raw","message_type":"ping","payload":{},"uid":0}"#;
    raw.write_all(&(malformed.len() as u32).to_be_bytes())
        .expect("write bounded frame prefix");
    raw.write_all(malformed)
        .expect("write malformed JSON envelope");
    let mut response_header = [0u8; 4];
    raw.read_exact(&mut response_header)
        .expect("read typed error frame header");
    let response_size = u32::from_be_bytes(response_header) as usize;
    assert!(response_size <= endfield_unix_ipc::MAX_FRAME_SIZE);
    let mut response_body = vec![0u8; response_size];
    raw.read_exact(&mut response_body)
        .expect("read typed malformed-request response");
    let response: endfield_unix_ipc::IpcResponse =
        serde_json::from_slice(&response_body).expect("typed error response");
    assert_eq!(
        response.payload,
        endfield_unix_ipc::ResponsePayload::Error {
            code: endfield_unix_ipc::ErrorCode::MalformedRequest
        }
    );
    drop(raw);

    let mut disconnected = UnixStream::connect(daemon.socket_path())
        .expect("connect a client that disconnects mid-frame");
    disconnected
        .write_all(&[0, 0])
        .expect("send a truncated frame prefix");
    drop(disconnected);

    let mut slow_client = UnixStream::connect(daemon.socket_path())
        .expect("connect a slow client before signal shutdown");
    slow_client
        .write_all(&[0, 0])
        .expect("send only part of the frame prefix");
    thread::sleep(Duration::from_millis(25));

    let after_malformed = run_cli(&cli, &runtime_dir, &["service", "ping"]);
    assert_eq!(after_malformed.status.code(), Some(0));
    daemon.stop(Signal::SIGTERM);
    drop(slow_client);

    let unavailable = run_cli(&cli, &runtime_dir, &["service", "status"]);
    assert_eq!(unavailable.status.code(), Some(75));
    assert!(!String::from_utf8_lossy(&unavailable.stdout).contains("Daemon: running"));

    let mut sigint_daemon = start_daemon(&daemon_binary, &directory, &runtime_dir);
    wait_for_cli_ping(&cli, &runtime_dir);
    sigint_daemon.stop(Signal::SIGINT);
    let unavailable = run_cli(&cli, &runtime_dir, &["service", "ping"]);
    assert_eq!(unavailable.status.code(), Some(75));
}

fn make_runtime_dir(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("runtime");
    fs::create_dir(&path).expect("create runtime directory");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .expect("restrict runtime directory");
    path
}

fn target_directory() -> PathBuf {
    std::env::current_exe()
        .expect("integration test executable")
        .parent()
        .and_then(|path| path.parent())
        .expect("target debug directory")
        .to_path_buf()
}

fn start_daemon(binary: &Path, directory: &TempDir, runtime_dir: &Path) -> ManagedDaemon {
    let child = Command::new(binary)
        .env_clear()
        .env("HOME", directory.path().join("home"))
        .env("XDG_STATE_HOME", directory.path().join("state"))
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .arg("--foreground")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the real daemon subprocess");
    ManagedDaemon {
        child,
        runtime_dir: runtime_dir.to_path_buf(),
    }
}

fn run_cli(binary: &Path, runtime_dir: &Path, arguments: &[&str]) -> Output {
    Command::new(binary)
        .env_clear()
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .args(arguments)
        .output()
        .expect("run the real CLI subprocess")
}

fn wait_for_cli_ping(binary: &Path, runtime_dir: &Path) {
    let deadline = Instant::now() + READY_LIMIT;
    loop {
        let output = run_cli(binary, runtime_dir, &["service", "ping"]);
        if output.status.success() {
            return;
        }
        assert!(Instant::now() < deadline, "daemon IPC did not become ready");
        thread::sleep(Duration::from_millis(20));
    }
}
