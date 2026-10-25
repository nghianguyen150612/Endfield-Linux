//! Injectable monotonic clock boundary for one-shot telemetry sessions.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use endfield_unix_telemetry::{ClockDomainId, MonotonicTimestamp};

static NEXT_SESSION_SEQUENCE: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSessionError {
    SessionIdentifierExhausted,
    ElapsedTimeOverflow,
}

/// Starts a new, explicitly scoped monotonic timeline for each collection.
pub trait TelemetryClockFactory: Send + Sync {
    type Session: MonotonicClockSession;

    fn start_session(&self) -> Result<Self::Session, ClockSessionError>;
}

/// All readings in a collection use one session and therefore one domain.
/// Test implementations can advance their timestamps without sleeping.
pub trait MonotonicClockSession: Send {
    fn domain(&self) -> ClockDomainId;
    fn now(&mut self) -> Result<MonotonicTimestamp, ClockSessionError>;
    fn wait(&mut self, duration: Duration);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTelemetryClockFactory;

impl TelemetryClockFactory for LinuxTelemetryClockFactory {
    type Session = SystemClockSession;

    fn start_session(&self) -> Result<Self::Session, ClockSessionError> {
        let sequence = NEXT_SESSION_SEQUENCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| ClockSessionError::SessionIdentifierExhausted)?;
        let process_id = std::process::id();
        let domain_value = (u64::from(process_id) << 32) | u64::from(sequence);
        let domain = ClockDomainId::new(domain_value)
            .map_err(|_| ClockSessionError::SessionIdentifierExhausted)?;
        Ok(SystemClockSession {
            domain,
            started: Instant::now(),
        })
    }
}

pub struct SystemClockSession {
    domain: ClockDomainId,
    started: Instant,
}

impl MonotonicClockSession for SystemClockSession {
    fn domain(&self) -> ClockDomainId {
        self.domain
    }

    fn now(&mut self) -> Result<MonotonicTimestamp, ClockSessionError> {
        let elapsed_nanos = u64::try_from(self.started.elapsed().as_nanos())
            .map_err(|_| ClockSessionError::ElapsedTimeOverflow)?;
        Ok(MonotonicTimestamp::new(self.domain, elapsed_nanos))
    }

    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
