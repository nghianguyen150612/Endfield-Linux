use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::Level;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

use crate::{Component, DiagnosticEvent, DiagnosticsError, EventCode, LogStore, Severity};

const DEFAULT_QUEUE_CAPACITY: usize = 256;
static FALLBACK_NOTICE_WRITTEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordDisposition {
    Queued,
    Filtered,
}

#[derive(Clone)]
pub struct LogHandle {
    sender: SyncSender<WorkerMessage>,
    minimum_severity: Severity,
    dropped_events: Arc<AtomicU64>,
}

impl LogHandle {
    /// Enqueues a typed event without waiting for file or stderr I/O.
    pub fn try_record(
        &self,
        event: DiagnosticEvent,
    ) -> Result<RecordDisposition, DiagnosticsError> {
        event.validate()?;
        if !event.severity().is_at_least(self.minimum_severity) {
            return Ok(RecordDisposition::Filtered);
        }
        match self.sender.try_send(WorkerMessage::Event(event)) {
            Ok(()) => Ok(RecordDisposition::Queued),
            Err(TrySendError::Full(_)) => {
                self.dropped_events.fetch_add(1, Ordering::Relaxed);
                Err(DiagnosticsError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => Err(DiagnosticsError::WorkerStopped),
        }
    }

    pub fn dropped_event_count(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Waits until events already in the queue have been written. Do not call
    /// this from the GTK event loop.
    pub fn flush(&self, timeout: Duration) -> Result<(), DiagnosticsError> {
        let deadline = deadline_after(timeout);
        let (sender, receiver) = mpsc::sync_channel(1);
        enqueue_control_message(&self.sender, WorkerMessage::Flush(sender), deadline)?;
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| DiagnosticsError::Timeout)
    }
}

pub struct LogWorker {
    handle: LogHandle,
    join: Option<JoinHandle<()>>,
}

impl LogWorker {
    pub fn start_with_store(
        store: LogStore,
        queue_capacity: usize,
        minimum_severity: Severity,
    ) -> Result<Self, DiagnosticsError> {
        Self::start_with_initializer(move || Ok(store), queue_capacity, minimum_severity)
    }

    /// Resolves and initializes the XDG log location on the worker thread.
    pub fn start_discovering(
        queue_capacity: usize,
        minimum_severity: Severity,
    ) -> Result<Self, DiagnosticsError> {
        Self::start_with_initializer(LogStore::from_environment, queue_capacity, minimum_severity)
    }

    fn start_with_initializer<F>(
        initialize_store: F,
        queue_capacity: usize,
        minimum_severity: Severity,
    ) -> Result<Self, DiagnosticsError>
    where
        F: FnOnce() -> Result<LogStore, DiagnosticsError> + Send + 'static,
    {
        if queue_capacity == 0 {
            return Err(DiagnosticsError::InvalidQueueCapacity);
        }
        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let dropped_events = Arc::new(AtomicU64::new(0));
        let worker_dropped = dropped_events.clone();
        let join = thread::Builder::new()
            .name("endfield-unix-diagnostics".to_owned())
            .spawn(move || worker_main(receiver, initialize_store, worker_dropped))
            .map_err(|_| DiagnosticsError::WorkerStart)?;

        Ok(Self {
            handle: LogHandle {
                sender,
                minimum_severity,
                dropped_events,
            },
            join: Some(join),
        })
    }

    pub fn handle(&self) -> LogHandle {
        self.handle.clone()
    }

    /// Stops after draining queued events and waits for the worker to exit.
    /// Call after the GTK event loop has returned, not while it is active.
    pub fn shutdown(mut self, timeout: Duration) -> Result<(), DiagnosticsError> {
        let deadline = deadline_after(timeout);
        let (sender, receiver) = mpsc::sync_channel(1);
        enqueue_control_message(
            &self.handle.sender,
            WorkerMessage::Shutdown(sender),
            deadline,
        )?;
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| DiagnosticsError::Timeout)?;
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| DiagnosticsError::WorkerStopped)?;
        }
        Ok(())
    }
}

impl Drop for LogWorker {
    fn drop(&mut self) {
        if self.join.is_some() {
            let (sender, _receiver) = mpsc::sync_channel(1);
            let _ = self.handle.sender.try_send(WorkerMessage::Shutdown(sender));
            // Dropping JoinHandle detaches the worker. This path never waits on
            // filesystem operations; explicit shutdown is available to tests.
            self.join.take();
        }
    }
}

enum WorkerMessage {
    Event(DiagnosticEvent),
    Flush(SyncSender<()>),
    Shutdown(SyncSender<()>),
}

fn enqueue_control_message(
    sender: &SyncSender<WorkerMessage>,
    mut message: WorkerMessage,
    deadline: Instant,
) -> Result<(), DiagnosticsError> {
    loop {
        match sender.try_send(message) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(DiagnosticsError::WorkerStopped),
            Err(TrySendError::Full(returned)) => {
                message = returned;
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(DiagnosticsError::Timeout);
                }
                thread::sleep(remaining.min(Duration::from_millis(1)));
            }
        }
    }
}

fn deadline_after(timeout: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(timeout).unwrap_or(now)
}

enum WorkerSink {
    File(LogStore),
    Disabled,
}

fn worker_main<F>(
    receiver: Receiver<WorkerMessage>,
    initialize_store: F,
    _dropped_events: Arc<AtomicU64>,
) where
    F: FnOnce() -> Result<LogStore, DiagnosticsError>,
{
    let mut sink = match initialize_store().and_then(|store| {
        store.initialize()?;
        Ok(store)
    }) {
        Ok(store) => WorkerSink::File(store),
        Err(error) => {
            write_fallback_notice(error_kind_code(&error));
            WorkerSink::Disabled
        }
    };

    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Event(event) => write_event(&mut sink, &event),
            WorkerMessage::Flush(acknowledge) => {
                let _ = acknowledge.send(());
            }
            WorkerMessage::Shutdown(acknowledge) => {
                let _ = acknowledge.send(());
                break;
            }
        }
    }
}

fn write_event(sink: &mut WorkerSink, event: &DiagnosticEvent) {
    if let WorkerSink::File(store) = sink {
        if store.append_event(event).is_ok() {
            return;
        }
        write_fallback_notice("log_write_failed");
        *sink = WorkerSink::Disabled;
    }
}

fn write_fallback_notice(code: &str) {
    if FALLBACK_NOTICE_WRITTEN.swap(true, Ordering::Relaxed) {
        return;
    }
    let message = format!(
        "endfield-unix: persistent diagnostics unavailable ({code}); continuing without file logging.\n"
    );
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(message.as_bytes());
}

fn error_kind_code(error: &DiagnosticsError) -> &'static str {
    match error {
        DiagnosticsError::HomeUnavailable => "state_path_unavailable",
        DiagnosticsError::UnsupportedPlatform => "unsupported_platform",
        DiagnosticsError::InvalidPath(_) => "invalid_path",
        DiagnosticsError::UnsafePath(_) => "unsafe_path",
        DiagnosticsError::Io { kind, .. } => match kind {
            crate::IoFailureKind::PermissionDenied => "permission_denied",
            crate::IoFailureKind::DiskFull => "disk_full",
            crate::IoFailureKind::AlreadyExists => "already_exists",
            crate::IoFailureKind::NotFound => "path_missing",
            crate::IoFailureKind::Other => "io_failure",
        },
        DiagnosticsError::OutputExists => "output_exists",
        DiagnosticsError::EventTooLarge => "event_too_large",
        DiagnosticsError::InvalidRetentionPolicy => "invalid_retention_policy",
        DiagnosticsError::InvalidQueueCapacity => "invalid_queue_capacity",
        DiagnosticsError::InvalidEvent(_) => "invalid_event",
        DiagnosticsError::Serialization => "serialization_failure",
        DiagnosticsError::ReportTooLarge => "report_too_large",
        DiagnosticsError::QueueFull => "queue_full",
        DiagnosticsError::WorkerStopped => "worker_stopped",
        DiagnosticsError::WorkerStart => "worker_start_failed",
        DiagnosticsError::Timeout => "worker_timeout",
    }
}

#[derive(Clone)]
struct SafeLifecycleLayer {
    handle: LogHandle,
}

impl<S> Layer<S> for SafeLifecycleLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        if metadata.target() != "endfield_unix::lifecycle" {
            return;
        }
        let (code, component) = match metadata.name() {
            "app_starting" => (EventCode::AppStarting, Component::Application),
            "gui_initialized" => (EventCode::GuiInitialized, Component::Gui),
            "gui_startup_failed" => (EventCode::GuiStartupFailed, Component::Gui),
            "window_closed_normally" => (EventCode::WindowClosedNormally, Component::Gui),
            "diagnostics_unavailable" => {
                (EventCode::DiagnosticsUnavailable, Component::Diagnostics)
            }
            _ => return,
        };
        let severity = match *metadata.level() {
            Level::ERROR => Severity::Error,
            Level::WARN => Severity::Warn,
            Level::INFO => Severity::Info,
            Level::DEBUG => Severity::Debug,
            Level::TRACE => Severity::Trace,
        };
        let event = DiagnosticEvent::new(severity, component, code);
        let _ = self.handle.try_record(event);
        // No event fields or formatted message are visited or retained.
    }
}

struct GlobalLogging {
    worker: LogWorker,
    subscriber_installed: bool,
}

static GLOBAL_LOGGING: OnceLock<Option<GlobalLogging>> = OnceLock::new();

/// Installs the one process-wide, allowlist-only tracing subscriber. Directory
/// creation and file writes happen on its bounded background worker.
pub fn initialize_global_logging() -> Result<(), DiagnosticsError> {
    let logging = GLOBAL_LOGGING.get_or_init(|| {
        let worker = LogWorker::start_discovering(DEFAULT_QUEUE_CAPACITY, Severity::Info).ok()?;
        let subscriber = tracing_subscriber::registry().with(SafeLifecycleLayer {
            handle: worker.handle(),
        });
        let subscriber_installed = tracing::subscriber::set_global_default(subscriber).is_ok();
        Some(GlobalLogging {
            worker,
            subscriber_installed,
        })
    });
    logging
        .as_ref()
        .map(|_| ())
        .ok_or(DiagnosticsError::WorkerStart)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    AppStarting,
    GuiInitialized,
    GuiStartupFailed,
    WindowClosedNormally,
    DiagnosticsUnavailable,
}

pub fn record_lifecycle(event: LifecycleEvent) {
    let Some(Some(logging)) = GLOBAL_LOGGING.get() else {
        let _ = event;
        write_fallback_notice("logger_not_initialized");
        return;
    };

    if logging.subscriber_installed {
        match event {
            LifecycleEvent::AppStarting => tracing::event!(
                name: "app_starting",
                target: "endfield_unix::lifecycle",
                Level::INFO,
                {}
            ),
            LifecycleEvent::GuiInitialized => tracing::event!(
                name: "gui_initialized",
                target: "endfield_unix::lifecycle",
                Level::INFO,
                {}
            ),
            LifecycleEvent::GuiStartupFailed => tracing::event!(
                name: "gui_startup_failed",
                target: "endfield_unix::lifecycle",
                Level::ERROR,
                {}
            ),
            LifecycleEvent::WindowClosedNormally => tracing::event!(
                name: "window_closed_normally",
                target: "endfield_unix::lifecycle",
                Level::INFO,
                {}
            ),
            LifecycleEvent::DiagnosticsUnavailable => tracing::event!(
                name: "diagnostics_unavailable",
                target: "endfield_unix::lifecycle",
                Level::WARN,
                {}
            ),
        }
    } else {
        let (code, component, severity) = lifecycle_parts(event);
        let _ = logging
            .worker
            .handle()
            .try_record(DiagnosticEvent::new(severity, component, code));
    }
}

pub fn flush_global_logging(timeout: Duration) -> Result<(), DiagnosticsError> {
    if let Some(Some(logging)) = GLOBAL_LOGGING.get() {
        logging.worker.handle().flush(timeout)
    } else {
        Ok(())
    }
}

pub fn global_dropped_event_count() -> u64 {
    GLOBAL_LOGGING
        .get()
        .and_then(Option::as_ref)
        .map_or(0, |logging| logging.worker.handle().dropped_event_count())
}

pub fn global_logging_initialized() -> bool {
    GLOBAL_LOGGING.get().is_some_and(Option::is_some)
}

fn lifecycle_parts(event: LifecycleEvent) -> (EventCode, Component, Severity) {
    match event {
        LifecycleEvent::AppStarting => (
            EventCode::AppStarting,
            Component::Application,
            Severity::Info,
        ),
        LifecycleEvent::GuiInitialized => {
            (EventCode::GuiInitialized, Component::Gui, Severity::Info)
        }
        LifecycleEvent::GuiStartupFailed => {
            (EventCode::GuiStartupFailed, Component::Gui, Severity::Error)
        }
        LifecycleEvent::WindowClosedNormally => (
            EventCode::WindowClosedNormally,
            Component::Gui,
            Severity::Info,
        ),
        LifecycleEvent::DiagnosticsUnavailable => (
            EventCode::DiagnosticsUnavailable,
            Component::Diagnostics,
            Severity::Warn,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn tracing_layer_uses_static_lifecycle_metadata_and_ignores_all_fields() {
        let directory = tempdir().expect("temporary directory");
        let store = LogStore::at_directory(directory.path().join("state/endfield-unix/logs"))
            .expect("log store");
        let worker =
            LogWorker::start_with_store(store.clone(), 8, Severity::Info).expect("logging worker");
        let subscriber = tracing_subscriber::registry().with(SafeLifecycleLayer {
            handle: worker.handle(),
        });
        let dispatch = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            tracing::event!(
                name: "app_starting",
                target: "endfield_unix::lifecycle",
                Level::INFO,
                token = "synthetic-tracing-token",
                command_line = "endfield-unix --password synthetic-password"
            );
            tracing::event!(
                name: "unapproved_event",
                target: "endfield_unix::lifecycle",
                Level::ERROR,
                token = "another-secret"
            );
        });

        worker
            .shutdown(Duration::from_secs(2))
            .expect("shutdown logger");
        let read = store.read_events().expect("read events");
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.events[0].code(), EventCode::AppStarting);
        let json = serde_json::to_string(&read.events[0]).expect("serialize safe event");
        assert!(!json.contains("synthetic-tracing-token"));
        assert!(!json.contains("synthetic-password"));
        assert!(!json.contains("another-secret"));
    }
}
