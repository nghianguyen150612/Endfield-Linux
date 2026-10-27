//! Safe, unprivileged management of the `endfield-unixd` systemd user unit.

use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use nix::unistd::{User, geteuid};

pub const UNIT_NAME: &str = "endfield-unixd.service";
pub const UNIT_CONTENT: &str = "# Managed by endfield-unix service systemd install (P020).\n\
[Unit]\n\
Description=Endfield-Unix User Daemon\n\
Documentation=https://github.com/nghianguyen150612/Endfield-Linux\n\
StartLimitIntervalSec=60\n\
StartLimitBurst=3\n\
\n\
[Service]\n\
Type=notify\n\
NotifyAccess=main\n\
ExecStart=%h/.local/bin/endfield-unixd --foreground\n\
Restart=on-failure\n\
RestartSec=3s\n\
TimeoutStartSec=15s\n\
TimeoutStopSec=10s\n\
KillSignal=SIGTERM\n\
KillMode=control-group\n\
UMask=0077\n\
NoNewPrivileges=true\n\
\n\
[Install]\n\
WantedBy=default.target\n";

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_COMMAND_OUTPUT: usize = 16 * 1024;
const MAX_BINARY_SIZE: u64 = 512 * 1024 * 1024;
const SERVICE_ID: &str = "endfield-unixd.service";
const OWNER_MAGIC: &str = "ENDFIELD-UNIX-P020-OWNER-V1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagerIssue {
    NonLinux,
    NonSystemdInit,
    ContainerWithoutManager,
    SystemctlMissing,
    UserManagerUnavailable,
    PermissionDenied,
    ProbeFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagerAvailability {
    Available,
    Unavailable(ManagerIssue),
}

impl fmt::Display for ManagerAvailability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Available => "available",
            Self::Unavailable(ManagerIssue::NonLinux) => {
                "unsupported platform (systemd user services require Linux)"
            }
            Self::Unavailable(ManagerIssue::NonSystemdInit) => {
                "unavailable (Linux is running another service manager)"
            }
            Self::Unavailable(ManagerIssue::ContainerWithoutManager) => {
                "unavailable (container has no reachable user manager)"
            }
            Self::Unavailable(ManagerIssue::SystemctlMissing) => {
                "unavailable (systemctl is not installed)"
            }
            Self::Unavailable(ManagerIssue::UserManagerUnavailable) => {
                "unavailable (systemd is installed but the user manager cannot be reached)"
            }
            Self::Unavailable(ManagerIssue::PermissionDenied) => {
                "unavailable (user-manager access was denied)"
            }
            Self::Unavailable(ManagerIssue::ProbeFailed) => {
                "unavailable (user-manager probe failed)"
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemdError {
    UnsupportedPlatform,
    RootNotSupported,
    SystemdToolsUnavailable,
    UserManagerUnavailable,
    UnsafePath,
    InvalidExecutable,
    Conflict,
    ActiveService,
    InstallationBusy,
    NotApplicationOwned,
    InvalidManagerResponse,
    CommandTimedOut,
    CommandFailed,
    Io,
}

impl fmt::Display for SystemdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => "systemd user-service installation requires Linux",
            Self::RootNotSupported => "systemd user-service management must run as an unprivileged user",
            Self::SystemdToolsUnavailable => "systemd tools are unavailable; manual foreground daemon use remains supported",
            Self::UserManagerUnavailable => "the systemd user manager is unavailable; no service-manager action was completed",
            Self::UnsafePath => "a systemd installation path is unsafe or has an unexpected owner",
            Self::InvalidExecutable => "the daemon source must be an absolute, user-owned executable regular file outside target/debug",
            Self::Conflict => "an existing destination differs from the generated installation; use --replace to save a recoverable backup",
            Self::ActiveService => "the daemon or service is active; stop it before replacing or uninstalling it",
            Self::InstallationBusy => "another service installation operation is already in progress",
            Self::NotApplicationOwned => "the installed files do not match the P020 ownership record; no files were removed",
            Self::InvalidManagerResponse => "systemd returned an invalid machine-readable unit status",
            Self::CommandTimedOut => "the bounded systemd command timed out",
            Self::CommandFailed => "the systemd operation failed; inspect the user journal for details",
            Self::Io => "a safe systemd installation file operation failed",
        })
    }
}

impl std::error::Error for SystemdError {}

#[derive(Clone, Debug)]
pub struct SystemdPaths {
    home: PathBuf,
    config_home: PathBuf,
    bin_directory: PathBuf,
    state_directory: PathBuf,
}

impl SystemdPaths {
    fn current_user() -> Result<Self, SystemdError> {
        let uid = geteuid();
        if uid.is_root() {
            return Err(SystemdError::RootNotSupported);
        }
        let user = User::from_uid(uid)
            .map_err(|_| SystemdError::UnsafePath)?
            .ok_or(SystemdError::UnsafePath)?;
        let home = fs::canonicalize(user.dir).map_err(|_| SystemdError::UnsafePath)?;
        let home_metadata = fs::symlink_metadata(&home).map_err(|_| SystemdError::UnsafePath)?;
        if !home_metadata.is_dir() || home_metadata.uid() != uid.as_raw() {
            return Err(SystemdError::UnsafePath);
        }
        let config_home = match env::var_os("XDG_CONFIG_HOME") {
            Some(value) => {
                let path = PathBuf::from(value);
                validate_absolute_path(&path)?;
                path
            }
            None => home.join(".config"),
        };
        let state_home = match env::var_os("XDG_STATE_HOME") {
            Some(value) => {
                let path = PathBuf::from(value);
                validate_absolute_path(&path)?;
                path
            }
            None => home.join(".local/state"),
        };
        Ok(Self {
            bin_directory: home.join(".local/bin"),
            state_directory: state_home.join("endfield-unix/systemd"),
            home,
            config_home,
        })
    }

    pub fn unit_path(&self) -> PathBuf {
        self.config_home.join("systemd/user").join(UNIT_NAME)
    }

    pub fn daemon_path(&self) -> PathBuf {
        self.bin_directory.join("endfield-unixd")
    }

    fn ownership_path(&self) -> PathBuf {
        self.state_directory.join("ownership-v1")
    }

    fn lock_path(&self) -> PathBuf {
        self.state_directory.join("install.lock")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallResult {
    pub binary_installed: bool,
    pub unit_installed: bool,
    pub manager_reloaded: bool,
    pub manager: ManagerAvailability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallResult {
    pub daemon_removed: bool,
    pub manager_reloaded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitManagerState {
    pub load_state: String,
    pub enabled_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: u32,
    pub result: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitFileState {
    Absent,
    Installed,
    ModifiedOrUnsafe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceStatus {
    pub manager: ManagerAvailability,
    pub unit_file: UnitFileState,
    pub unit: Option<UnitManagerState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceOperation {
    Enable,
    Disable,
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunFailure {
    Spawn,
    Timeout,
    OutputLimit,
}

trait CommandRunner {
    fn run(
        &self,
        program: &Path,
        arguments: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, RunFailure>;
}

struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(
        &self,
        program: &Path,
        arguments: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, RunFailure> {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in [
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "SYSTEMD_BUS_ADDRESS",
        ] {
            if let Some(value) = env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().map_err(|_| RunFailure::Spawn)?;
        collect_bounded_output(&mut child, timeout)
    }
}

fn collect_bounded_output(
    child: &mut Child,
    timeout: Duration,
) -> Result<CommandOutput, RunFailure> {
    let stdout = child.stdout.take().ok_or(RunFailure::Spawn)?;
    let stderr = child.stderr.take().ok_or(RunFailure::Spawn)?;
    let stdout_reader = thread::Builder::new()
        .name("endfield-systemctl-stdout".to_owned())
        .spawn(move || read_bounded(stdout))
        .map_err(|_| RunFailure::Spawn)?;
    let stderr_reader = thread::Builder::new()
        .name("endfield-systemctl-stderr".to_owned())
        .spawn(move || read_bounded(stderr))
        .map_err(|_| RunFailure::Spawn)?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|_| RunFailure::Spawn)? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(RunFailure::Timeout);
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = stdout_reader.join().map_err(|_| RunFailure::Spawn)?;
    let stderr = stderr_reader.join().map_err(|_| RunFailure::Spawn)?;
    let stdout = stdout?;
    let stderr = stderr?;
    Ok(CommandOutput {
        status: status.code(),
        stdout,
        stderr,
    })
}

fn read_bounded(mut reader: impl Read) -> Result<Vec<u8>, RunFailure> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_COMMAND_OUTPUT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RunFailure::Spawn)?;
    if bytes.len() > MAX_COMMAND_OUTPUT {
        return Err(RunFailure::OutputLimit);
    }
    Ok(bytes)
}

pub struct SystemdUserManager {
    paths: SystemdPaths,
    systemctl: Option<PathBuf>,
    systemd_analyze: Option<PathBuf>,
    runner: Box<dyn CommandRunner + Send + Sync>,
    ipc_reachable: Box<dyn Fn() -> bool + Send + Sync>,
}

impl SystemdUserManager {
    pub fn current() -> Result<Self, SystemdError> {
        Ok(Self {
            paths: SystemdPaths::current_user()?,
            systemctl: trusted_tool("systemctl"),
            systemd_analyze: trusted_tool("systemd-analyze"),
            runner: Box::new(ProcessRunner),
            ipc_reachable: Box::new(ipc_is_reachable),
        })
    }

    #[cfg(test)]
    fn with_paths_and_runner(
        paths: SystemdPaths,
        runner: Box<dyn CommandRunner + Send + Sync>,
    ) -> Self {
        Self {
            paths,
            systemctl: Some(PathBuf::from("/usr/bin/systemctl")),
            systemd_analyze: Some(PathBuf::from("/usr/bin/systemd-analyze")),
            runner,
            ipc_reachable: Box::new(|| false),
        }
    }

    pub fn unit_path(&self) -> PathBuf {
        self.paths.unit_path()
    }

    pub fn daemon_path(&self) -> PathBuf {
        self.paths.daemon_path()
    }

    pub fn detect(&self) -> ManagerAvailability {
        if !cfg!(target_os = "linux") {
            return ManagerAvailability::Unavailable(ManagerIssue::NonLinux);
        }
        let Some(systemctl) = self.systemctl.as_deref() else {
            return ManagerAvailability::Unavailable(classify_without_systemctl());
        };
        match self.runner.run(
            systemctl,
            &["--user", "show", "--property=Version"],
            STATUS_TIMEOUT,
        ) {
            Ok(output) if output.status == Some(0) => ManagerAvailability::Available,
            Ok(output) => ManagerAvailability::Unavailable(classify_manager_error(&output)),
            Err(RunFailure::Spawn) => {
                ManagerAvailability::Unavailable(ManagerIssue::SystemctlMissing)
            }
            Err(_) => ManagerAvailability::Unavailable(ManagerIssue::ProbeFailed),
        }
    }

    pub fn install(
        &self,
        daemon_source: Option<&Path>,
        replace: bool,
    ) -> Result<InstallResult, SystemdError> {
        if !cfg!(target_os = "linux") {
            return Err(SystemdError::UnsupportedPlatform);
        }
        self.systemctl
            .as_deref()
            .ok_or(SystemdError::SystemdToolsUnavailable)?;
        let analyzer = self
            .systemd_analyze
            .as_deref()
            .ok_or(SystemdError::SystemdToolsUnavailable)?;
        validate_owner_directories(&self.paths, true)?;
        let binary_destination = self.paths.daemon_path();
        let unit_destination = self.paths.unit_path();
        let source = match daemon_source {
            Some(path) => path.to_path_buf(),
            None => default_daemon_source(&binary_destination, &self.paths)
                .ok_or(SystemdError::InvalidExecutable)?,
        };
        validate_daemon_source(&source, &binary_destination)?;

        let manager = self.detect();
        let _lock = InstallLock::acquire(&self.paths.lock_path())?;
        let installed_unit = inspect_regular_file(&unit_destination, UNIT_CONTENT.len() as u64)?;
        let installed_binary = inspect_regular_metadata(&binary_destination, MAX_BINARY_SIZE)?;
        let previous_ownership =
            if inspect_regular_metadata(&self.paths.ownership_path(), 1024)?.is_some() {
                Some(read_ownership_manifest(&self.paths)?)
            } else {
                None
            };
        let unit_is_exact = installed_unit.as_ref().is_some_and(|(bytes, metadata)| {
            bytes == UNIT_CONTENT.as_bytes() && metadata.mode() & 0o077 == 0
        });
        let source_is_destination = same_path(&source, &binary_destination);
        let binary_matches_source = source_is_destination
            || installed_binary.is_some() && same_file_bytes(&source, &binary_destination);
        let unit_owned = installed_unit.as_ref().is_some_and(|(_, metadata)| {
            previous_ownership
                .as_ref()
                .is_some_and(|ownership| ownership.unit == FileIdentity::from_metadata(metadata))
        });
        let binary_owned = installed_binary.as_ref().is_some_and(|metadata| {
            previous_ownership
                .as_ref()
                .is_some_and(|ownership| ownership.daemon == FileIdentity::from_metadata(metadata))
        });
        let binary_is_source = binary_matches_source
            && binary_owned
            && installed_binary
                .as_ref()
                .is_some_and(|metadata| metadata.mode() & 0o111 != 0);

        if (installed_unit.is_some() && !(unit_is_exact && unit_owned)
            || installed_binary.is_some() && !binary_is_source)
            && !replace
        {
            return Err(SystemdError::Conflict);
        }
        if replace && (installed_unit.is_some() || installed_binary.is_some()) {
            if manager != ManagerAvailability::Available {
                return Err(SystemdError::UserManagerUnavailable);
            }
            self.ensure_unit_inactive()?;
            if (self.ipc_reachable)() {
                return Err(SystemdError::ActiveService);
            }
        }

        let prior_unit = if installed_unit.is_some() && !(unit_is_exact && unit_owned) {
            Some(save_recoverable_backup(&unit_destination)?)
        } else {
            None
        };
        let prior_binary = if installed_binary.is_some() && !binary_is_source {
            match save_recoverable_backup(&binary_destination) {
                Ok(backup) => Some(backup),
                Err(error) => {
                    if let Some(backup) = prior_unit.as_deref() {
                        let _ = fs::remove_file(backup);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };

        let mut binary_installed = false;
        let mut unit_installed = false;
        if !binary_is_source {
            if let Err(error) =
                install_executable(&source, &binary_destination, installed_binary.is_some())
            {
                restore_backup(prior_unit.as_deref(), &unit_destination, false);
                restore_backup(prior_binary.as_deref(), &binary_destination, false);
                return Err(error);
            }
            binary_installed = true;
        }

        let validation = match TemporaryUnit::create(&unit_destination, UNIT_CONTENT.as_bytes()) {
            Ok(validation) => validation,
            Err(error) => {
                restore_backup(prior_unit.as_deref(), &unit_destination, false);
                restore_backup(
                    prior_binary.as_deref(),
                    &binary_destination,
                    binary_installed,
                );
                if previous_ownership.is_some() {
                    let _ = write_ownership_manifest(&self.paths);
                } else {
                    let _ = fs::remove_file(self.paths.ownership_path());
                }
                return Err(error);
            }
        };
        // Verify the exact unit after the trusted daemon is present at its
        // final path. This does not require a live user manager.
        let verification = self.runner.run(
            analyzer,
            &["verify", validation.path_string()],
            COMMAND_TIMEOUT,
        );
        drop(validation);
        let verification = match verification {
            Ok(output) if output.status == Some(0) => Ok(()),
            Ok(_) => Err(SystemdError::CommandFailed),
            Err(error) => Err(map_run_failure(error)),
        };
        if let Err(error) = verification {
            restore_backup(prior_unit.as_deref(), &unit_destination, false);
            restore_backup(
                prior_binary.as_deref(),
                &binary_destination,
                binary_installed,
            );
            if previous_ownership.is_some() {
                let _ = write_ownership_manifest(&self.paths);
            } else {
                let _ = fs::remove_file(self.paths.ownership_path());
            }
            return Err(error);
        }

        let result = (|| {
            if !(unit_is_exact && unit_owned) {
                atomic_install(
                    &unit_destination,
                    UNIT_CONTENT.as_bytes(),
                    0o600,
                    installed_unit.is_some(),
                )?;
                unit_installed = true;
            }
            write_ownership_manifest(&self.paths)?;
            Ok::<(), SystemdError>(())
        })();
        if let Err(error) = result {
            restore_backup(prior_unit.as_deref(), &unit_destination, unit_installed);
            restore_backup(
                prior_binary.as_deref(),
                &binary_destination,
                binary_installed,
            );
            if previous_ownership.is_some() {
                let _ = write_ownership_manifest(&self.paths);
            } else {
                let _ = fs::remove_file(self.paths.ownership_path());
            }
            return Err(error);
        }

        let mut manager_reloaded = false;
        if manager == ManagerAvailability::Available {
            self.run_systemctl(&["daemon-reload"], COMMAND_TIMEOUT)?;
            manager_reloaded = true;
        }
        Ok(InstallResult {
            binary_installed,
            unit_installed,
            manager_reloaded,
            manager,
        })
    }

    pub fn uninstall(&self) -> Result<UninstallResult, SystemdError> {
        let manager = self.detect();
        if manager != ManagerAvailability::Available {
            return Err(SystemdError::UserManagerUnavailable);
        }
        validate_owner_directories(&self.paths, false)?;
        let unit_path = self.paths.unit_path();
        let Some((unit_bytes, unit_metadata)) =
            inspect_regular_file(&unit_path, UNIT_CONTENT.len() as u64)?
        else {
            return Err(SystemdError::NotApplicationOwned);
        };
        if unit_bytes != UNIT_CONTENT.as_bytes() {
            return Err(SystemdError::NotApplicationOwned);
        }
        let ownership = read_ownership_manifest(&self.paths)?;
        if ownership.unit != FileIdentity::from_metadata(&unit_metadata) {
            return Err(SystemdError::NotApplicationOwned);
        }

        let _lock = InstallLock::acquire(&self.paths.lock_path())?;
        let (_, current_unit_metadata) =
            inspect_regular_file(&unit_path, UNIT_CONTENT.len() as u64)?
                .ok_or(SystemdError::NotApplicationOwned)?;
        let current_ownership = read_ownership_manifest(&self.paths)?;
        if current_ownership.unit != FileIdentity::from_metadata(&current_unit_metadata)
            || current_ownership.unit != ownership.unit
        {
            return Err(SystemdError::NotApplicationOwned);
        }
        self.ensure_unit_inactive()?;
        if (self.ipc_reachable)() {
            return Err(SystemdError::ActiveService);
        }
        self.run_systemctl(&["disable", SERVICE_ID], COMMAND_TIMEOUT)?;
        fs::remove_file(&unit_path).map_err(map_io_error)?;
        let binary_path = self.paths.daemon_path();
        let mut daemon_removed = false;
        if let Some(metadata) = inspect_regular_metadata(&binary_path, MAX_BINARY_SIZE)? {
            if ownership.daemon == FileIdentity::from_metadata(&metadata) {
                fs::remove_file(&binary_path).map_err(map_io_error)?;
                daemon_removed = true;
            }
        }
        let _ = fs::remove_file(self.paths.ownership_path());
        self.run_systemctl(&["daemon-reload"], COMMAND_TIMEOUT)?;
        Ok(UninstallResult {
            daemon_removed,
            manager_reloaded: true,
        })
    }

    pub fn operate(&self, operation: ServiceOperation) -> Result<(), SystemdError> {
        if self.detect() != ManagerAvailability::Available {
            return Err(SystemdError::UserManagerUnavailable);
        }
        let _lock = InstallLock::acquire(&self.paths.lock_path())?;
        self.verify_owned_installation()?;
        let verb = match operation {
            ServiceOperation::Enable => "enable",
            ServiceOperation::Disable => "disable",
            ServiceOperation::Start => "start",
            ServiceOperation::Stop => "stop",
            ServiceOperation::Restart => "restart",
        };
        self.run_systemctl(&[verb, SERVICE_ID], COMMAND_TIMEOUT)?;
        Ok(())
    }

    pub fn status(&self) -> ServiceStatus {
        let manager = self.detect();
        let unit_file = match safe_parent_chain(&self.paths.unit_path()) {
            Ok(false) => UnitFileState::Absent,
            Err(_) => UnitFileState::ModifiedOrUnsafe,
            Ok(true) => match self.verify_owned_unit() {
                Ok(()) => UnitFileState::Installed,
                Err(SystemdError::NotApplicationOwned) => UnitFileState::ModifiedOrUnsafe,
                Err(_) => UnitFileState::ModifiedOrUnsafe,
            },
        };
        let unit = if manager == ManagerAvailability::Available {
            self.systemctl
                .as_deref()
                .and_then(|systemctl| {
                    self.runner
                        .run(
                            systemctl,
                            &[
                                "--user",
                                "show",
                                "--no-pager",
                                "--property=LoadState",
                                "--property=UnitFileState",
                                "--property=ActiveState",
                                "--property=SubState",
                                "--property=MainPID",
                                "--property=Result",
                                SERVICE_ID,
                            ],
                            STATUS_TIMEOUT,
                        )
                        .ok()
                })
                .and_then(|output| parse_unit_state(&output).ok())
        } else {
            None
        };
        ServiceStatus {
            manager,
            unit_file,
            unit,
        }
    }

    fn ensure_unit_inactive(&self) -> Result<(), SystemdError> {
        let state = self.read_unit_state()?;
        if state.main_pid != 0
            || matches!(
                state.active_state.as_str(),
                "active" | "activating" | "reloading" | "deactivating"
            )
        {
            return Err(SystemdError::ActiveService);
        }
        if !matches!(state.active_state.as_str(), "inactive" | "failed" | "") {
            return Err(SystemdError::InvalidManagerResponse);
        }
        Ok(())
    }

    fn verify_owned_unit(&self) -> Result<(), SystemdError> {
        let (unit_bytes, unit_metadata) =
            inspect_regular_file(&self.paths.unit_path(), UNIT_CONTENT.len() as u64)?
                .ok_or(SystemdError::NotApplicationOwned)?;
        validate_owner_directories(&self.paths, false)?;
        let owner = read_ownership_manifest(&self.paths)?;
        if unit_bytes != UNIT_CONTENT.as_bytes()
            || unit_metadata.mode() & 0o077 != 0
            || owner.unit != FileIdentity::from_metadata(&unit_metadata)
        {
            return Err(SystemdError::NotApplicationOwned);
        }
        Ok(())
    }

    fn verify_owned_installation(&self) -> Result<(), SystemdError> {
        self.verify_owned_unit()?;
        let daemon_metadata = inspect_regular_metadata(&self.paths.daemon_path(), MAX_BINARY_SIZE)?
            .ok_or(SystemdError::NotApplicationOwned)?;
        let owner = read_ownership_manifest(&self.paths)?;
        if owner.daemon != FileIdentity::from_metadata(&daemon_metadata) {
            return Err(SystemdError::NotApplicationOwned);
        }
        if daemon_metadata.mode() & 0o111 == 0 {
            return Err(SystemdError::NotApplicationOwned);
        }
        Ok(())
    }

    fn read_unit_state(&self) -> Result<UnitManagerState, SystemdError> {
        let output = self.run_systemctl(
            &[
                "show",
                "--no-pager",
                "--property=LoadState",
                "--property=UnitFileState",
                "--property=ActiveState",
                "--property=SubState",
                "--property=MainPID",
                "--property=Result",
                SERVICE_ID,
            ],
            STATUS_TIMEOUT,
        )?;
        parse_unit_state(&output)
    }

    fn run_systemctl(
        &self,
        arguments: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, SystemdError> {
        let path = self
            .systemctl
            .as_deref()
            .ok_or(SystemdError::SystemdToolsUnavailable)?;
        let output = self
            .runner
            .run(path, &with_user_option(arguments), timeout)
            .map_err(map_run_failure)?;
        if output.status != Some(0) {
            return Err(SystemdError::CommandFailed);
        }
        Ok(output)
    }
}

fn with_user_option<'a>(arguments: &'a [&'a str]) -> Vec<&'a str> {
    std::iter::once("--user")
        .chain(arguments.iter().copied())
        .collect()
}

fn trusted_tool(name: &str) -> Option<PathBuf> {
    [Path::new("/usr/bin"), Path::new("/bin")]
        .into_iter()
        .map(|directory| directory.join(name))
        .find_map(|path| {
            let canonical = fs::canonicalize(path).ok()?;
            let metadata = fs::metadata(&canonical).ok()?;
            (metadata.is_file()
                && metadata.uid() == 0
                && metadata.mode() & 0o022 == 0
                && metadata.mode() & 0o111 != 0)
                .then_some(canonical)
        })
}

fn classify_without_systemctl() -> ManagerIssue {
    let init = fs::read_to_string("/proc/1/comm").unwrap_or_default();
    classify_without_systemctl_with_context(init.trim(), is_container())
}

fn classify_without_systemctl_with_context(init: &str, container: bool) -> ManagerIssue {
    if container {
        ManagerIssue::ContainerWithoutManager
    } else if init == "systemd" {
        ManagerIssue::SystemctlMissing
    } else {
        ManagerIssue::NonSystemdInit
    }
}

fn classify_manager_error(output: &CommandOutput) -> ManagerIssue {
    let init = fs::read_to_string("/proc/1/comm").unwrap_or_default();
    classify_manager_error_with_context(output, is_container(), init.trim())
}

fn classify_manager_error_with_context(
    output: &CommandOutput,
    container: bool,
    init: &str,
) -> ManagerIssue {
    let message = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if message.contains("permission denied")
        || message.contains("access denied")
        || message.contains("not authorized")
    {
        ManagerIssue::PermissionDenied
    } else if container {
        ManagerIssue::ContainerWithoutManager
    } else if !init.is_empty() && init != "systemd" {
        ManagerIssue::NonSystemdInit
    } else if message.contains("failed to connect to bus")
        || message.contains("no medium found")
        || message.contains("connection refused")
        || message.contains("no such file or directory")
    {
        ManagerIssue::UserManagerUnavailable
    } else {
        ManagerIssue::ProbeFailed
    }
}

fn is_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

fn parse_unit_state(output: &CommandOutput) -> Result<UnitManagerState, SystemdError> {
    if output.status != Some(0) {
        return Err(SystemdError::CommandFailed);
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| SystemdError::InvalidManagerResponse)?;
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or(SystemdError::InvalidManagerResponse)?;
        if !matches!(
            key,
            "LoadState" | "UnitFileState" | "ActiveState" | "SubState" | "MainPID" | "Result"
        ) || values.insert(key, value).is_some()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        {
            return Err(SystemdError::InvalidManagerResponse);
        }
    }
    let get = |key| {
        values
            .get(key)
            .copied()
            .ok_or(SystemdError::InvalidManagerResponse)
    };
    Ok(UnitManagerState {
        load_state: get("LoadState")?.to_owned(),
        enabled_state: get("UnitFileState")?.to_owned(),
        active_state: get("ActiveState")?.to_owned(),
        sub_state: get("SubState")?.to_owned(),
        main_pid: get("MainPID")?
            .parse()
            .map_err(|_| SystemdError::InvalidManagerResponse)?,
        result: get("Result")?.to_owned(),
    })
}

fn validate_absolute_path(path: &Path) -> Result<(), SystemdError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(SystemdError::UnsafePath);
    }
    Ok(())
}

fn validate_owner_directories(paths: &SystemdPaths, create: bool) -> Result<(), SystemdError> {
    let uid = geteuid().as_raw();
    ensure_directory(&paths.home, uid, false, 0o700)?;
    ensure_directory(&paths.config_home, uid, create, 0o700)?;
    ensure_directory(&paths.config_home.join("systemd"), uid, create, 0o700)?;
    ensure_directory(&paths.config_home.join("systemd/user"), uid, create, 0o700)?;
    ensure_directory(&paths.bin_directory, uid, create, 0o700)?;
    if create {
        ensure_directory(&paths.state_directory, uid, true, 0o700)?;
    } else if paths.state_directory.exists() {
        ensure_directory(&paths.state_directory, uid, false, 0o700)?;
    }
    Ok(())
}

fn ensure_directory(path: &Path, uid: u32, create: bool, mode: u32) -> Result<(), SystemdError> {
    validate_absolute_path(path)?;
    let mut current = PathBuf::from("/");
    for component in path.components().skip(1) {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                let root_owned = metadata.uid() == 0;
                let sticky_root_directory = root_owned && metadata.mode() & 0o1000 != 0;
                if !metadata.is_dir()
                    || (!root_owned && metadata.uid() != uid)
                    || (metadata.mode() & 0o022 != 0 && !sticky_root_directory)
                {
                    return Err(SystemdError::UnsafePath);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                fs::DirBuilder::new()
                    .mode(mode)
                    .create(&current)
                    .map_err(map_io_error)?;
                fs::set_permissions(&current, fs::Permissions::from_mode(mode))
                    .map_err(map_io_error)?;
                let metadata = fs::symlink_metadata(&current).map_err(map_io_error)?;
                if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
                    return Err(SystemdError::UnsafePath);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SystemdError::UnsafePath);
            }
            Err(_) => return Err(SystemdError::UnsafePath),
        }
    }
    Ok(())
}

/// Validates every existing parent without following links. `false` means an
/// ancestor is absent, so no destination can exist there yet.
fn safe_parent_chain(path: &Path) -> Result<bool, SystemdError> {
    let parent = path.parent().ok_or(SystemdError::UnsafePath)?;
    let uid = geteuid().as_raw();
    let mut current = PathBuf::from("/");
    for component in parent.components().skip(1) {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(SystemdError::UnsafePath),
        };
        let root_owned = metadata.uid() == 0;
        let sticky_root_directory = root_owned && metadata.mode() & 0o1000 != 0;
        if !metadata.is_dir()
            || (!root_owned && metadata.uid() != uid)
            || (metadata.mode() & 0o022 != 0 && !sticky_root_directory)
        {
            return Err(SystemdError::UnsafePath);
        }
    }
    Ok(true)
}

fn default_daemon_source(destination: &Path, paths: &SystemdPaths) -> Option<PathBuf> {
    let current = env::current_exe().ok()?;
    let parent = current.parent()?;
    let candidate = parent.join("endfield-unixd");
    if let Ok(canonical) = fs::canonicalize(&candidate) {
        if !is_target_debug(&canonical) {
            return Some(candidate);
        }
    }
    let installed = inspect_regular_metadata(destination, MAX_BINARY_SIZE).ok()??;
    let ownership = read_ownership_manifest(paths).ok()?;
    (ownership.daemon == FileIdentity::from_metadata(&installed)).then(|| destination.to_path_buf())
}

fn validate_daemon_source(source: &Path, destination: &Path) -> Result<(), SystemdError> {
    validate_absolute_path(source).map_err(|_| SystemdError::InvalidExecutable)?;
    let metadata = fs::symlink_metadata(source).map_err(|_| SystemdError::InvalidExecutable)?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.nlink() != 1
        || metadata.mode() & 0o111 == 0
        || metadata.mode() & 0o022 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_BINARY_SIZE
        || (is_target_debug(source) && !same_path(source, destination))
    {
        return Err(SystemdError::InvalidExecutable);
    }
    Ok(())
}

fn is_target_debug(path: &Path) -> bool {
    let components = path
        .components()
        .map(|component| component.as_os_str())
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair[0] == "target" && pair[1] == "debug")
}

fn same_path(left: &Path, right: &Path) -> bool {
    fs::canonicalize(left)
        .ok()
        .zip(fs::canonicalize(right).ok())
        .is_some_and(|(a, b)| a == b)
}

fn same_file_bytes(left: &Path, right: &Path) -> bool {
    let Ok(Some((mut left, _))) = open_owned_regular_file(left, MAX_BINARY_SIZE) else {
        return false;
    };
    let Ok(Some((mut right, _))) = open_owned_regular_file(right, MAX_BINARY_SIZE) else {
        return false;
    };
    let mut left_buffer = [0u8; 16 * 1024];
    let mut right_buffer = [0u8; 16 * 1024];
    loop {
        let left_size = match left.read(&mut left_buffer) {
            Ok(size) => size,
            Err(_) => return false,
        };
        let right_size = match right.read(&mut right_buffer) {
            Ok(size) => size,
            Err(_) => return false,
        };
        if left_size != right_size || left_buffer[..left_size] != right_buffer[..right_size] {
            return false;
        }
        if left_size == 0 {
            return true;
        }
    }
}

fn open_owned_regular_file(
    path: &Path,
    max_size: u64,
) -> Result<Option<(File, fs::Metadata)>, SystemdError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SystemdError::UnsafePath),
    };
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.nlink() != 1
        || metadata.mode() & 0o022 != 0
        || metadata.len() > max_size
    {
        return Err(SystemdError::UnsafePath);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(map_io_error)?;
    let opened_metadata = file.metadata().map_err(map_io_error)?;
    if !opened_metadata.is_file()
        || opened_metadata.dev() != metadata.dev()
        || opened_metadata.ino() != metadata.ino()
        || opened_metadata.uid() != geteuid().as_raw()
        || opened_metadata.nlink() != 1
        || opened_metadata.mode() & 0o022 != 0
        || opened_metadata.len() > max_size
    {
        return Err(SystemdError::UnsafePath);
    }
    Ok(Some((file, opened_metadata)))
}

fn inspect_regular_metadata(
    path: &Path,
    max_size: u64,
) -> Result<Option<fs::Metadata>, SystemdError> {
    Ok(open_owned_regular_file(path, max_size)?.map(|(_, metadata)| metadata))
}

fn inspect_regular_file(
    path: &Path,
    max_size: u64,
) -> Result<Option<(Vec<u8>, fs::Metadata)>, SystemdError> {
    let Some((file, metadata)) = open_owned_regular_file(path, max_size)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(max_size + 1)
        .read_to_end(&mut bytes)
        .map_err(map_io_error)?;
    if bytes.len() as u64 > max_size {
        return Err(SystemdError::UnsafePath);
    }
    Ok(Some((bytes, metadata)))
}

fn install_executable(
    source: &Path,
    destination: &Path,
    replace: bool,
) -> Result<(), SystemdError> {
    let (source_file, source_metadata) =
        open_owned_regular_file(source, MAX_BINARY_SIZE)?.ok_or(SystemdError::InvalidExecutable)?;
    if source_metadata.mode() & 0o111 == 0 || source_metadata.len() == 0 {
        return Err(SystemdError::InvalidExecutable);
    }
    let temp = temporary_sibling(destination)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)
        .map_err(map_io_error)?;
    let copied = match io::copy(
        &mut source_file.take(MAX_BINARY_SIZE + 1),
        &mut destination_file,
    ) {
        Ok(copied) => copied,
        Err(error) => {
            drop(destination_file);
            let _ = fs::remove_file(&temp);
            return Err(map_io_error(error));
        }
    };
    if copied == 0 || copied > MAX_BINARY_SIZE || copied != source_metadata.len() {
        drop(destination_file);
        let _ = fs::remove_file(&temp);
        return Err(SystemdError::InvalidExecutable);
    }
    if let Err(error) = destination_file.set_permissions(fs::Permissions::from_mode(0o755)) {
        drop(destination_file);
        let _ = fs::remove_file(&temp);
        return Err(map_io_error(error));
    }
    if let Err(error) = destination_file.sync_all() {
        drop(destination_file);
        let _ = fs::remove_file(&temp);
        return Err(map_io_error(error));
    }
    drop(destination_file);
    commit_temporary(&temp, destination, replace)
}

fn atomic_install(path: &Path, bytes: &[u8], mode: u32, replace: bool) -> Result<(), SystemdError> {
    let temp = temporary_sibling(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)
        .map_err(map_io_error)?;
    if let Err(error) = file
        .write_all(bytes)
        .and_then(|()| file.set_permissions(fs::Permissions::from_mode(mode)))
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(map_io_error(error));
    }
    drop(file);
    commit_temporary(&temp, path, replace)
}

fn commit_temporary(temp: &Path, destination: &Path, replace: bool) -> Result<(), SystemdError> {
    let result = if replace {
        fs::rename(temp, destination).map_err(map_io_error)
    } else {
        fs::hard_link(temp, destination).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                SystemdError::Conflict
            } else {
                map_io_error(error)
            }
        })
    };
    if result.is_err() || !replace {
        let _ = fs::remove_file(temp);
    }
    result?;
    if let Some(parent) = destination.parent() {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(map_io_error)?;
    }
    Ok(())
}

fn temporary_sibling(path: &Path) -> Result<PathBuf, SystemdError> {
    let name = path.file_name().ok_or(SystemdError::UnsafePath)?;
    let nonce = TEMPORARY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_file_name(format!(
        ".{}.p020-{}-{nonce}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    if fs::symlink_metadata(&temp).is_ok() {
        return Err(SystemdError::Conflict);
    }
    Ok(temp)
}

static TEMPORARY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn save_recoverable_backup(path: &Path) -> Result<PathBuf, SystemdError> {
    let name = path.file_name().ok_or(SystemdError::UnsafePath)?;
    let backup = path.with_file_name(format!("{}.p020-backup", name.to_string_lossy()));
    if fs::symlink_metadata(&backup).is_ok() {
        return Err(SystemdError::Conflict);
    }
    let _ = inspect_regular_metadata(path, MAX_BINARY_SIZE)?.ok_or(SystemdError::Conflict)?;
    fs::hard_link(path, &backup).map_err(map_io_error)?;
    Ok(backup)
}

fn restore_backup(backup: Option<&Path>, destination: &Path, installed: bool) {
    let Some(backup) = backup else {
        if installed {
            let _ = fs::remove_file(destination);
        }
        return;
    };
    if installed {
        let _ = fs::remove_file(destination);
        let _ = fs::rename(backup, destination);
    } else {
        let _ = fs::remove_file(backup);
    }
}

struct TemporaryUnit(PathBuf);

impl TemporaryUnit {
    fn create(unit_path: &Path, bytes: &[u8]) -> Result<Self, SystemdError> {
        let parent = unit_path.parent().ok_or(SystemdError::UnsafePath)?;
        let nonce = TEMPORARY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = parent.join(format!(
            "endfield-unixd-p020-validate-{}-{nonce}.service",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(map_io_error)?;
        if let Err(error) = file
            .write_all(bytes)
            .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o600)))
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(map_io_error(error));
        }
        drop(file);
        Ok(Self(path))
    }

    fn path_string(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

impl Drop for TemporaryUnit {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
    mode: u32,
}

impl FileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            size: metadata.size(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
            mode: metadata.mode(),
        }
    }

    fn encode(self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            self.dev,
            self.ino,
            self.size,
            self.mtime,
            self.mtime_nsec,
            self.ctime,
            self.ctime_nsec,
            self.mode
        )
    }

    fn decode(value: &str) -> Result<Self, SystemdError> {
        let fields = value.split(':').collect::<Vec<_>>();
        if fields.len() != 8 {
            return Err(SystemdError::NotApplicationOwned);
        }
        Ok(Self {
            dev: fields[0]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            ino: fields[1]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            size: fields[2]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            mtime: fields[3]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            mtime_nsec: fields[4]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            ctime: fields[5]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            ctime_nsec: fields[6]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
            mode: fields[7]
                .parse()
                .map_err(|_| SystemdError::NotApplicationOwned)?,
        })
    }
}

struct OwnershipManifest {
    daemon: FileIdentity,
    unit: FileIdentity,
}

fn write_ownership_manifest(paths: &SystemdPaths) -> Result<(), SystemdError> {
    let daemon = fs::metadata(paths.daemon_path()).map_err(map_io_error)?;
    let unit = fs::metadata(paths.unit_path()).map_err(map_io_error)?;
    let contents = format!(
        "{OWNER_MAGIC}\nuid={}\ndaemon={}\nunit={}\n",
        geteuid().as_raw(),
        FileIdentity::from_metadata(&daemon).encode(),
        FileIdentity::from_metadata(&unit).encode(),
    );
    atomic_install(&paths.ownership_path(), contents.as_bytes(), 0o600, true)
}

fn read_ownership_manifest(paths: &SystemdPaths) -> Result<OwnershipManifest, SystemdError> {
    let path = paths.ownership_path();
    let Some((bytes, metadata)) = inspect_regular_file(&path, 1024)? else {
        return Err(SystemdError::NotApplicationOwned);
    };
    if metadata.mode() & 0o077 != 0 {
        return Err(SystemdError::NotApplicationOwned);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| SystemdError::NotApplicationOwned)?;
    let mut lines = text.lines();
    let expected_uid = format!("uid={}", geteuid().as_raw());
    if lines.next() != Some(OWNER_MAGIC) || lines.next() != Some(expected_uid.as_str()) {
        return Err(SystemdError::NotApplicationOwned);
    }
    let daemon = lines
        .next()
        .and_then(|line| line.strip_prefix("daemon="))
        .ok_or(SystemdError::NotApplicationOwned)?;
    let unit = lines
        .next()
        .and_then(|line| line.strip_prefix("unit="))
        .ok_or(SystemdError::NotApplicationOwned)?;
    if lines.next().is_some() {
        return Err(SystemdError::NotApplicationOwned);
    }
    Ok(OwnershipManifest {
        daemon: FileIdentity::decode(daemon)?,
        unit: FileIdentity::decode(unit)?,
    })
}

fn map_run_failure(error: RunFailure) -> SystemdError {
    match error {
        RunFailure::Timeout => SystemdError::CommandTimedOut,
        RunFailure::Spawn | RunFailure::OutputLimit => SystemdError::CommandFailed,
    }
}

fn map_io_error(error: io::Error) -> SystemdError {
    match error.kind() {
        io::ErrorKind::AlreadyExists => SystemdError::Conflict,
        _ => SystemdError::Io,
    }
}

fn ipc_is_reachable() -> bool {
    endfield_unix_ipc::Client::from_environment(Duration::from_millis(400))
        .and_then(|client| client.ping())
        .is_ok()
}

struct InstallLock(File);

impl InstallLock {
    fn acquire(path: &Path) -> Result<Self, SystemdError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(map_io_error)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(map_io_error)?;
        let metadata = file.metadata().map_err(map_io_error)?;
        if !metadata.is_file()
            || metadata.uid() != geteuid().as_raw()
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err(SystemdError::UnsafePath);
        }
        // Never wait without a deadline while another local management call holds the lock.
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                SystemdError::InstallationBusy
            } else {
                SystemdError::Io
            }
        })?;
        Ok(Self(file))
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    use tempfile::{TempDir, tempdir};

    use super::*;

    type RecordedCall = (PathBuf, Vec<String>);

    #[derive(Clone, Default)]
    struct MockRunner {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        responses: Arc<Mutex<VecDeque<Result<CommandOutput, RunFailure>>>>,
    }

    impl MockRunner {
        fn responding(responses: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
            }
        }
    }

    impl CommandRunner for MockRunner {
        fn run(
            &self,
            program: &Path,
            arguments: &[&str],
            _: Duration,
        ) -> Result<CommandOutput, RunFailure> {
            self.calls.lock().unwrap().push((
                program.to_path_buf(),
                arguments.iter().map(|value| (*value).to_owned()).collect(),
            ));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(success_output(Vec::new())))
        }
    }

    fn success_output(stdout: Vec<u8>) -> CommandOutput {
        CommandOutput {
            status: Some(0),
            stdout,
            stderr: Vec::new(),
        }
    }

    fn test_manager(
        temp: &TempDir,
        runner: MockRunner,
    ) -> (SystemdUserManager, SystemdPaths, PathBuf) {
        let home = temp.path().join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let config = home.join(".config");
        fs::create_dir(&config).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
        let paths = SystemdPaths {
            home: home.clone(),
            config_home: config,
            bin_directory: home.join(".local/bin"),
            state_directory: home.join(".local/state/endfield-unix/systemd"),
        };
        let source = temp.path().join("endfield-unixd-release");
        fs::write(&source, b"test daemon binary").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        let manager = SystemdUserManager::with_paths_and_runner(paths.clone(), Box::new(runner));
        (manager, paths, source)
    }

    fn output_with_status(status: i32, stdout: &str) -> CommandOutput {
        CommandOutput {
            status: Some(status),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn installation_lock_rejects_a_concurrent_management_operation_without_waiting() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("install.lock");
        let first = InstallLock::acquire(&path).unwrap();
        assert!(matches!(
            InstallLock::acquire(&path),
            Err(SystemdError::InstallationBusy)
        ));
        drop(first);
        assert!(InstallLock::acquire(&path).is_ok());
    }

    #[test]
    fn unit_template_has_notify_readiness_and_conservative_cgroup_cleanup() {
        assert_eq!(
            UNIT_CONTENT,
            include_str!("../../../../packaging/systemd/user/endfield-unixd.service")
        );
        assert!(UNIT_CONTENT.contains("Type=notify\nNotifyAccess=main"));
        assert!(UNIT_CONTENT.contains("ExecStart=%h/.local/bin/endfield-unixd --foreground"));
        assert!(UNIT_CONTENT.contains("KillMode=control-group"));
        assert!(UNIT_CONTENT.contains("Restart=on-failure\nRestartSec=3s"));
        assert!(UNIT_CONTENT.contains("StartLimitIntervalSec=60\nStartLimitBurst=3"));
        assert!(UNIT_CONTENT.contains("UMask=0077\nNoNewPrivileges=true"));
        assert!(!UNIT_CONTENT.contains("KillMode=process"));
        assert!(!UNIT_CONTENT.contains("KillMode=none"));
        assert!(!UNIT_CONTENT.contains("target/debug"));
    }

    #[test]
    fn structured_unit_status_parser_rejects_extra_and_malformed_output() {
        let output = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\nMainPID=42\nResult=success\n",
        );
        let state = parse_unit_state(&output).unwrap();
        assert_eq!(state.active_state, "active");
        assert_eq!(state.main_pid, 42);
        assert_eq!(
            parse_unit_state(&output_with_status(0, "MainPID=12\nUnknown=value\n")),
            Err(SystemdError::InvalidManagerResponse)
        );
    }

    #[test]
    fn service_operations_use_only_fixed_systemctl_user_arguments() {
        let temp = tempdir().unwrap();
        let mut responses = vec![
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(Vec::new()),
        ];
        for _ in 0..5 {
            responses.push(success_output(b"Version=256\n".to_vec()));
            responses.push(success_output(Vec::new()));
        }
        let runner = MockRunner::responding(responses);
        let (manager, _, source) = test_manager(&temp, runner.clone());
        manager.install(Some(&source), false).unwrap();
        for operation in [
            ServiceOperation::Enable,
            ServiceOperation::Disable,
            ServiceOperation::Start,
            ServiceOperation::Stop,
            ServiceOperation::Restart,
        ] {
            manager.operate(operation).unwrap();
        }
        let calls = runner.calls.lock().unwrap();
        let verbs = calls
            .iter()
            .filter(|(_, args)| {
                args.first().is_some_and(|value| value == "--user")
                    && args.get(1).is_some_and(|value| {
                        ["enable", "disable", "start", "stop", "restart"].contains(&value.as_str())
                    })
            })
            .map(|(_, args)| args[1].as_str())
            .collect::<Vec<_>>();
        assert_eq!(verbs, ["enable", "disable", "start", "stop", "restart"]);
        assert!(calls.iter().all(|(program, arguments)| {
            if program == Path::new("/usr/bin/systemctl") {
                arguments.first().is_some_and(|arg| arg == "--user")
            } else {
                program == Path::new("/usr/bin/systemd-analyze")
                    && arguments.first().is_some_and(|arg| arg == "verify")
            }
        }));
        assert!(!calls.iter().any(|(_, arguments)| {
            arguments
                .iter()
                .any(|arg| arg == "sudo" || arg.contains(';'))
        }));
    }

    #[test]
    fn install_is_idempotent_and_does_not_enable_or_start() {
        let temp = tempdir().unwrap();
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner.clone());
        let first = manager.install(Some(&source), false).unwrap();
        assert!(first.binary_installed);
        assert!(first.unit_installed);
        assert!(first.manager_reloaded);
        assert_eq!(
            fs::read(paths.unit_path()).unwrap(),
            UNIT_CONTENT.as_bytes()
        );
        assert_eq!(
            fs::metadata(paths.unit_path()).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(paths.daemon_path()).unwrap().mode() & 0o777,
            0o755
        );
        let second = manager.install(Some(&paths.daemon_path()), false).unwrap();
        assert!(!second.binary_installed);
        assert!(!second.unit_installed);
        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|(_, args)| {
            args.iter()
                .any(|value| value == "enable" || value == "start")
        }));
    }

    #[test]
    fn conflicting_unit_and_symlink_destinations_are_not_overwritten() {
        let temp = tempdir().unwrap();
        let runner = MockRunner::responding([success_output(b"Version=256\n".to_vec())]);
        let (manager, paths, source) = test_manager(&temp, runner);
        validate_owner_directories(&paths, true).unwrap();
        fs::write(paths.unit_path(), b"user content").unwrap();
        assert_eq!(
            manager.install(Some(&source), false),
            Err(SystemdError::Conflict)
        );
        fs::remove_file(paths.unit_path()).unwrap();
        let foreign = temp.path().join("foreign");
        fs::write(&foreign, b"user content").unwrap();
        std::os::unix::fs::symlink(&foreign, paths.unit_path()).unwrap();
        assert_eq!(
            manager.install(Some(&source), true),
            Err(SystemdError::UnsafePath)
        );
        assert_eq!(fs::read(&foreign).unwrap(), b"user content");
    }

    #[test]
    fn installer_rejects_relative_and_debug_daemon_paths() {
        let temp = tempdir().unwrap();
        let (_manager, paths, source) = test_manager(&temp, MockRunner::default());
        assert_eq!(
            validate_daemon_source(Path::new("relative"), &paths.daemon_path()),
            Err(SystemdError::InvalidExecutable)
        );
        let debug = temp.path().join("target/debug/endfield-unixd");
        fs::create_dir_all(debug.parent().unwrap()).unwrap();
        fs::write(&debug, b"debug binary").unwrap();
        fs::set_permissions(&debug, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            validate_daemon_source(&debug, &paths.daemon_path()),
            Err(SystemdError::InvalidExecutable)
        );
        assert!(validate_daemon_source(&source, &paths.daemon_path()).is_ok());
    }

    #[test]
    fn default_source_does_not_adopt_an_unowned_existing_destination() {
        let temp = tempdir().unwrap();
        let (_manager, paths, _) = test_manager(&temp, MockRunner::default());
        fs::create_dir_all(&paths.bin_directory).unwrap();
        fs::write(paths.daemon_path(), b"unrelated existing executable").unwrap();
        fs::set_permissions(paths.daemon_path(), fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            default_daemon_source(&paths.daemon_path(), &paths),
            None,
            "--replace must require a separate explicitly trusted source"
        );
    }

    #[test]
    fn generated_unit_matches_the_packaging_source_byte_for_byte() {
        assert_eq!(
            UNIT_CONTENT,
            include_str!("../../../../packaging/systemd/user/endfield-unixd.service")
        );
    }

    #[test]
    fn authorized_replacement_keeps_a_recoverable_prior_daemon() {
        let temp = tempdir().unwrap();
        let inactive = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=disabled\nActiveState=inactive\nSubState=dead\nMainPID=0\nResult=success\n",
        );
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(Vec::new()),
            success_output(b"Version=256\n".to_vec()),
            inactive,
            success_output(Vec::new()),
            success_output(Vec::new()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner);
        manager.install(Some(&source), false).unwrap();
        let previous = fs::read(paths.daemon_path()).unwrap();
        let replacement = temp.path().join("replacement daemon; user supplied name");
        fs::write(&replacement, b"replacement daemon binary").unwrap();
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();

        manager.install(Some(&replacement), true).unwrap();

        assert_eq!(
            fs::read(paths.daemon_path()).unwrap(),
            b"replacement daemon binary"
        );
        assert_eq!(
            fs::read(
                paths
                    .daemon_path()
                    .with_file_name("endfield-unixd.p020-backup")
            )
            .unwrap(),
            previous
        );
        assert_eq!(
            fs::read(paths.unit_path()).unwrap(),
            UNIT_CONTENT.as_bytes()
        );
    }

    #[test]
    fn explicit_unit_replacement_preserves_the_existing_user_unit() {
        let temp = tempdir().unwrap();
        let inactive = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=disabled\nActiveState=inactive\nSubState=dead\nMainPID=0\nResult=success\n",
        );
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            inactive,
            success_output(Vec::new()),
            success_output(Vec::new()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner);
        validate_owner_directories(&paths, true).unwrap();
        fs::write(paths.unit_path(), b"# existing user-managed unit\n").unwrap();

        manager.install(Some(&source), true).unwrap();

        assert_eq!(
            fs::read(paths.unit_path()).unwrap(),
            UNIT_CONTENT.as_bytes()
        );
        assert_eq!(
            fs::read(
                paths
                    .unit_path()
                    .with_file_name("endfield-unixd.service.p020-backup")
            )
            .unwrap(),
            b"# existing user-managed unit\n"
        );
    }

    #[test]
    fn uninstall_removes_only_verified_owned_files_after_inactive_check() {
        let temp = tempdir().unwrap();
        let inactive = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=disabled\nActiveState=inactive\nSubState=dead\nMainPID=0\nResult=success\n",
        );
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(Vec::new()),
            success_output(b"Version=256\n".to_vec()),
            inactive,
            success_output(Vec::new()),
            success_output(Vec::new()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner);
        manager.install(Some(&source), false).unwrap();
        let unrelated = paths.home.join("unrelated.txt");
        fs::write(&unrelated, b"preserve me").unwrap();

        let result = manager.uninstall().unwrap();

        assert!(result.daemon_removed);
        assert!(!paths.unit_path().exists());
        assert!(!paths.daemon_path().exists());
        assert_eq!(fs::read(unrelated).unwrap(), b"preserve me");
    }

    #[test]
    fn uninstall_retains_a_daemon_whose_permissions_changed_after_install() {
        let temp = tempdir().unwrap();
        let inactive = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=disabled\nActiveState=inactive\nSubState=dead\nMainPID=0\nResult=success\n",
        );
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(Vec::new()),
            success_output(b"Version=256\n".to_vec()),
            inactive,
            success_output(Vec::new()),
            success_output(Vec::new()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner);
        manager.install(Some(&source), false).unwrap();
        fs::set_permissions(paths.daemon_path(), fs::Permissions::from_mode(0o700)).unwrap();

        let result = manager.uninstall().unwrap();

        assert!(!result.daemon_removed);
        assert!(paths.daemon_path().exists());
        assert!(!paths.unit_path().exists());
    }

    #[test]
    fn active_unit_and_tampered_installs_are_not_removed_or_managed() {
        let temp = tempdir().unwrap();
        let active = output_with_status(
            0,
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\nMainPID=123\nResult=success\n",
        );
        let runner = MockRunner::responding([
            success_output(b"Version=256\n".to_vec()),
            success_output(Vec::new()),
            success_output(Vec::new()),
            success_output(b"Version=256\n".to_vec()),
            active,
            success_output(b"Version=256\n".to_vec()),
        ]);
        let (manager, paths, source) = test_manager(&temp, runner);
        manager.install(Some(&source), false).unwrap();
        assert_eq!(manager.uninstall(), Err(SystemdError::ActiveService));
        fs::write(paths.daemon_path(), b"replaced by an unrelated workflow").unwrap();
        assert_eq!(
            manager.operate(ServiceOperation::Start),
            Err(SystemdError::NotApplicationOwned)
        );
        assert!(paths.unit_path().exists());
    }

    #[test]
    fn systemctl_timeout_nonzero_exit_and_manager_unavailability_are_typed() {
        let temp = tempdir().unwrap();
        let runner = MockRunner::default();
        runner
            .responses
            .lock()
            .unwrap()
            .push_back(Err(RunFailure::Timeout));
        let (manager, _, _) = test_manager(&temp, runner);
        assert_eq!(
            manager.run_systemctl(&["start", SERVICE_ID], STATUS_TIMEOUT),
            Err(SystemdError::CommandTimedOut)
        );
        let failed = MockRunner::responding([output_with_status(1, "")]);
        let second_temp = tempdir().unwrap();
        let (manager, _, _) = test_manager(&second_temp, failed);
        assert_eq!(
            manager.run_systemctl(&["start", SERVICE_ID], STATUS_TIMEOUT),
            Err(SystemdError::CommandFailed)
        );
        assert_eq!(
            classify_without_systemctl_with_context("systemd", false),
            ManagerIssue::SystemctlMissing
        );
        assert_eq!(
            classify_without_systemctl_with_context("openrc", false),
            ManagerIssue::NonSystemdInit
        );
        assert_eq!(
            classify_without_systemctl_with_context("systemd", true),
            ManagerIssue::ContainerWithoutManager
        );
    }

    #[test]
    fn failed_and_unavailable_manager_states_remain_distinct() {
        assert_eq!(
            classify_manager_error_with_context(
                &CommandOutput {
                    status: Some(1),
                    stdout: Vec::new(),
                    stderr: b"Failed to connect to bus: No medium found".to_vec(),
                },
                false,
                "systemd",
            ),
            ManagerIssue::UserManagerUnavailable
        );
        assert_eq!(
            classify_manager_error_with_context(
                &CommandOutput {
                    status: Some(1),
                    stdout: Vec::new(),
                    stderr: b"Access denied".to_vec()
                },
                true,
                "init",
            ),
            ManagerIssue::PermissionDenied
        );
        assert_eq!(
            classify_manager_error_with_context(
                &CommandOutput {
                    status: Some(1),
                    stdout: Vec::new(),
                    stderr: b"Failed to connect to bus".to_vec(),
                },
                true,
                "systemd"
            ),
            ManagerIssue::ContainerWithoutManager
        );
        assert_eq!(
            parse_unit_state(&output_with_status(1, "")),
            Err(SystemdError::CommandFailed)
        );
    }
}
