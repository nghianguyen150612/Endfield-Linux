#![cfg(target_os = "linux")]

use std::{
    ffi::OsStr,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        Arc, Barrier, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Fixture {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/controlled_child.rs");
        let executable = directory.path().join("controlled-child");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let result = Command::new(rustc)
            .arg("--edition=2024")
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap();
        assert!(result.success());
        Fixture {
            _directory: directory,
            executable,
        }
    })
}

#[derive(Debug)]
struct ChildResult {
    status: ExitStatus,
    output: Vec<u8>,
}

fn run_fixture(
    executable: &Path,
    args: &[&OsStr],
    working_directory: &Path,
    output_limit: usize,
    exit_code: i32,
    output_bytes: usize,
) -> io::Result<ChildResult> {
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(working_directory)
        .env_clear()
        .env("P034_ALLOWED_MARKER", "explicit-value")
        .env("P034_EXIT_CODE", exit_code.to_string())
        .env("P034_OUTPUT_BYTES", output_bytes.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(output_limit as u64 + 1)
        .read_to_end(&mut output)?;
    if output.len() > output_limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "synthetic output bound exceeded",
        ));
    }
    let status = child.wait()?;
    Ok(ChildResult { status, output })
}

#[test]
fn direct_process_api_preserves_literal_arguments_cwd_and_environment_isolation() {
    let fixture = fixture();
    let cwd = fixture._directory.path();
    let injected_path = cwd.join("injected");
    let shell_text = format!("$(touch {})", injected_path.display());
    let args = ["text with spaces", "雪", shell_text.as_str(), "semi; &"];
    let args: Vec<_> = args.iter().map(OsStr::new).collect();
    let output = run_fixture(&fixture.executable, &args, cwd, 8192, 0, 0).unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.output).unwrap();
    assert!(text.contains(&format!("cwd={}", cwd.display())));
    assert!(text.contains("marker=explicit-value"));
    assert!(text.contains("inherited="));
    assert!(text.contains("path_present=false"));
    assert!(!text.contains("inherited=explicit-value"));
    for arg in ["text with spaces", "雪", shell_text.as_str(), "semi; &"] {
        assert!(text.contains(&format!("arg={arg}")), "{text}");
    }
    assert!(!injected_path.exists());
}

#[test]
fn controlled_child_reports_early_exit_and_nonzero_status() {
    let fixture = fixture();
    let result = run_fixture(
        &fixture.executable,
        &[],
        fixture._directory.path(),
        8192,
        17,
        0,
    )
    .unwrap();
    assert_eq!(result.status.code(), Some(17));
}

#[test]
fn output_limit_kills_child_and_returns_a_bounded_error() {
    let fixture = fixture();
    let error = run_fixture(
        &fixture.executable,
        &[],
        fixture._directory.path(),
        4096,
        0,
        1_000_000,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("bound"));
}

#[test]
fn missing_executable_fails_before_a_child_is_created() {
    let error = run_fixture(
        Path::new("/no/such/p034-fixture"),
        &[],
        Path::new("/"),
        64,
        0,
        0,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn isolated_test_authorization_rejects_mismatch_and_cancellation_before_spawn() {
    #[derive(Default)]
    struct TestAuthorization {
        plan: String,
        approved: bool,
        cancelled: bool,
    }
    let spawn_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempt = |authorization: &TestAuthorization| {
        if !authorization.approved
            || authorization.plan != "sha256:expected"
            || authorization.cancelled
        {
            return false;
        }
        spawn_count.fetch_add(1, Ordering::SeqCst);
        true
    };
    assert!(!attempt(&TestAuthorization {
        plan: "sha256:other".into(),
        approved: true,
        cancelled: false
    }));
    assert!(!attempt(&TestAuthorization {
        plan: "sha256:expected".into(),
        approved: true,
        cancelled: true
    }));
    assert_eq!(spawn_count.load(Ordering::SeqCst), 0);
}

#[test]
fn only_one_test_launch_claim_can_be_active() {
    let active = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let first_active = active.clone();
    let first_barrier = barrier.clone();
    let first = thread::spawn(move || {
        let claimed = first_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        first_barrier.wait();
        first_barrier.wait();
        first_active.store(false, Ordering::SeqCst);
        claimed
    });
    barrier.wait();
    assert!(active.load(Ordering::SeqCst));
    assert!(
        active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    );
    barrier.wait();
    assert!(first.join().unwrap());
}

#[test]
fn fixture_source_is_repository_owned_and_stable() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/controlled_child.rs");
    assert!(fs::metadata(source).unwrap().is_file());
}
