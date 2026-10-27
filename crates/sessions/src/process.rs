//! Operating-system process exit classification isolated from session state.

use std::io;
use std::process::{Child, ExitStatus};

use crate::{ExitOutcome, MonitorFailure};

/// Internal seam for deterministic monitor contracts. Production always uses
/// the owned `Child::try_wait` implementation below; tests can control
/// observation timing without adding PID attachment or process creation APIs.
pub(super) trait ChildMonitor {
    fn try_wait(&mut self, child: &mut Child) -> io::Result<Option<ExitStatus>>;
}

pub(super) struct OwnedChildMonitor;

impl ChildMonitor for OwnedChildMonitor {
    fn try_wait(&mut self, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        child.try_wait()
    }
}

pub(crate) fn exit_outcome(status: ExitStatus) -> ExitOutcome {
    if let Some(code) = status.code() {
        return ExitOutcome::Exited { code };
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitOutcome::Signaled { signal };
        }
    }
    ExitOutcome::StatusUnavailable
}

pub(crate) fn wait_failure(error: &std::io::Error) -> MonitorFailure {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ECHILD) {
        return MonitorFailure::OwnershipUnavailable;
    }
    MonitorFailure::WaitFailed
}
