use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

pub struct FixtureChild {
    pub child: Option<Child>,
    pub writer: Option<ChildStdin>,
    pub pid: u32,
}

impl FixtureChild {
    pub fn spawn() -> Self {
        let mut child = Command::new(std::env::current_exe().expect("test executable path"))
            .args([
                "--ignored",
                "--exact",
                "common::owned_child_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the fixed harmless test helper");
        let pid = child.id();
        let writer = child.stdin.take().expect("piped test helper stdin");
        Self {
            child: Some(child),
            writer: Some(writer),
            pid,
        }
    }

    pub fn transfer(&mut self) -> (Child, ChildStdin) {
        (
            self.child.take().expect("test child transfers once"),
            self.writer.take().expect("test control transfers once"),
        )
    }
}

impl Drop for FixtureChild {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.write_all(&[0]);
            drop(writer);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        } else {
            reap_test_child(self.pid);
        }
    }
}

pub fn reap_test_child(pid: u32) {
    let pid = Pid::from_raw(i32::try_from(pid).expect("test PID fits pid_t"));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {}
            Ok(_) | Err(Errno::ECHILD) => return,
            Err(error) => panic!("test helper reaping failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "test helper exceeded cleanup deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore]
fn owned_child_fixture() {
    let mut command = [0_u8; 1];
    if std::io::stdin().read_exact(&mut command).is_ok() {
        match command[0] {
            b'7' => std::process::exit(7),
            b's' => std::process::abort(),
            _ => {}
        }
    }
}
