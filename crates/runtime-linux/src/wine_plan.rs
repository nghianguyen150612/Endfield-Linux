//! Preparation-only managed Wine invocation plans.
//!
//! This module never starts a process. Its builder encodes the documented
//! native Wine argument model and verifies the exact filesystem objects seen
//! while preparing the preview. Callers must separately revalidate P025/P026,
//! P027, P028, P031, P032, and P029 evidence immediately before any future
//! process boundary.

use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

pub const MAX_ARGUMENTS: usize = 256;
pub const MAX_ARGUMENT_BYTES: usize = 32 * 1024;
pub const MAX_TOTAL_ARGUMENT_BYTES: usize = 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const DESKTOP_ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "PULSE_SERVER",
    "PIPEWIRE_REMOTE",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinePlanError {
    InvalidIdentifier,
    InvalidPath,
    RuntimeExecutableUnavailable,
    PrefixUnavailable,
    GameExecutableUnavailable,
    WorkingDirectoryUnavailable,
    RuntimePrefixAssociationMismatch,
    InvalidArguments,
}

/// Exact durable identities that approval would have to bind. These are
/// supplied only after the application revalidates the respective registries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationBinding {
    pub runtime_installation_id: String,
    pub runtime_pin_id: String,
    pub runtime_pin_revision: u64,
    pub runtime_reference_sha256: String,
    pub runtime_generation: u64,
    pub runtime_ownership_record_sha256: String,
    pub runtime_artifact_sha256: String,
    pub runtime_audit_report_sha256: String,
    pub prefix_id: String,
    pub prefix_registry_revision: u64,
    pub prefix_generation: u64,
    pub prefix_device: u64,
    pub prefix_inode: u64,
    pub prefix_runtime_pin_id: String,
    pub prefix_runtime_pin_revision: u64,
    pub prefix_runtime_reference_sha256: String,
    pub game_installation_id: String,
    pub game_registry_revision: u64,
    pub game_root_device: u64,
    pub game_root_inode: u64,
    pub game_executable_device: u64,
    pub game_executable_inode: u64,
}

impl std::fmt::Debug for ManagedWineInvocationPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedWineInvocationPlan")
            .field("adapter", &"managed_wine")
            .field("binding", &self.binding)
            .field("expected_executable", &"<redacted>")
            .field("native_arguments", &"<redacted>")
            .field("working_directory", &"<redacted>")
            .field("environment", &"<redacted>")
            .field(
                "runtime_executable_identity",
                &self.runtime_executable_identity,
            )
            .field("prefix_identity", &self.prefix_identity)
            .field("game_executable_identity", &self.game_executable_identity)
            .field("approval_required", &self.approval_required)
            .field("execution_supported", &self.execution_supported)
            .field("expected_effects", &self.expected_effects)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WineEnvironmentPolicy {
    /// A future executor must clear ambient variables and then copy only the
    /// desktop-session allowlist documented in `docs/GAME-LAUNCH.md`.
    ClearThenDesktopSessionAllowlist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessLifetimePolicy {
    /// P019 ownership is not integrated. A future adapter may wait for only
    /// its direct child; it must not claim wineserver descendants are owned.
    DirectChildOnlyNoDaemonOwnership,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedWineInvocationPlan {
    pub plan_id: String,
    pub adapter_id: &'static str,
    pub binding: InvocationBinding,
    pub expected_executable: PathBuf,
    /// Native argv entries after argv[0]: game executable followed by literal
    /// caller arguments. Values are never joined or shell-escaped.
    pub native_arguments: Vec<OsString>,
    pub working_directory: PathBuf,
    pub wine_prefix: PathBuf,
    pub environment_policy: WineEnvironmentPolicy,
    pub desktop_environment_allowlist: Vec<&'static str>,
    pub runtime_executable_identity: FileIdentity,
    pub prefix_identity: FileIdentity,
    pub game_executable_identity: FileIdentity,
    pub process_lifetime: ProcessLifetimePolicy,
    pub approval_required: bool,
    pub execution_supported: bool,
    pub expected_effects: Vec<&'static str>,
}

pub struct ManagedWineInvocationRequest {
    pub runtime_root: PathBuf,
    pub prefix: PathBuf,
    pub game_root: PathBuf,
    pub game_executable_relative: PathBuf,
    pub working_directory: PathBuf,
    pub arguments: Vec<OsString>,
    pub binding: InvocationBinding,
}

pub struct ManagedWineInvocationPlanBuilder;

impl ManagedWineInvocationPlanBuilder {
    /// Prepare the exact `bin/wine <game.exe> <args...>` native argv. The
    /// returned plan is never an authorization token and cannot be executed.
    pub fn prepare(
        request: ManagedWineInvocationRequest,
    ) -> Result<ManagedWineInvocationPlan, WinePlanError> {
        validate_identifier(&request.binding.runtime_installation_id, "wine-")?;
        validate_identifier(&request.binding.runtime_pin_id, "")?;
        validate_identifier(&request.binding.prefix_id, "")?;
        validate_identifier(&request.binding.prefix_runtime_pin_id, "")?;
        validate_identifier(&request.binding.game_installation_id, "")?;
        if request.binding.runtime_pin_revision == 0
            || request.binding.runtime_generation == 0
            || request.binding.prefix_registry_revision == 0
            || request.binding.prefix_generation == 0
            || request.binding.game_registry_revision == 0
            || request.binding.prefix_runtime_pin_id != request.binding.runtime_pin_id
            || request.binding.prefix_runtime_pin_revision != request.binding.runtime_pin_revision
            || request.binding.prefix_runtime_reference_sha256
                != request.binding.runtime_reference_sha256
            || !request
                .binding
                .runtime_reference_sha256
                .strip_prefix("sha256:")
                .is_some_and(valid_sha256)
            || !request
                .binding
                .prefix_runtime_reference_sha256
                .strip_prefix("sha256:")
                .is_some_and(valid_sha256)
            || !valid_sha256(&request.binding.runtime_ownership_record_sha256)
            || !valid_sha256(&request.binding.runtime_artifact_sha256)
            || !valid_sha256(&request.binding.runtime_audit_report_sha256)
        {
            return Err(WinePlanError::RuntimePrefixAssociationMismatch);
        }

        let runtime_root = checked_directory(&request.runtime_root)
            .ok_or(WinePlanError::RuntimeExecutableUnavailable)?;
        let expected_executable = runtime_root.join("bin/wine");
        let runtime_executable_identity = checked_executable(&expected_executable)
            .ok_or(WinePlanError::RuntimeExecutableUnavailable)?;
        let prefix = checked_directory(&request.prefix).ok_or(WinePlanError::PrefixUnavailable)?;
        let prefix_identity =
            identity(&fs::metadata(&prefix).map_err(|_| WinePlanError::PrefixUnavailable)?);
        if prefix_identity.device != request.binding.prefix_device
            || prefix_identity.inode != request.binding.prefix_inode
        {
            return Err(WinePlanError::PrefixUnavailable);
        }

        let game_root = checked_directory(&request.game_root)
            .ok_or(WinePlanError::GameExecutableUnavailable)?;
        validate_relative(&request.game_executable_relative)?;
        let game_executable = game_root.join(&request.game_executable_relative);
        let game_executable =
            checked_executable(&game_executable).ok_or(WinePlanError::GameExecutableUnavailable)?;
        let game_root_identity = identity(
            &fs::metadata(&game_root).map_err(|_| WinePlanError::GameExecutableUnavailable)?,
        );
        let game_executable_identity = identity(
            &fs::metadata(&game_executable_path(
                &game_root,
                &request.game_executable_relative,
            )?)
            .map_err(|_| WinePlanError::GameExecutableUnavailable)?,
        );
        if game_root_identity.device != request.binding.game_root_device
            || game_root_identity.inode != request.binding.game_root_inode
            || game_executable_identity.device != request.binding.game_executable_device
            || game_executable_identity.inode != request.binding.game_executable_inode
        {
            return Err(WinePlanError::GameExecutableUnavailable);
        }
        let working_directory = checked_directory(&request.working_directory)
            .ok_or(WinePlanError::WorkingDirectoryUnavailable)?;
        if !working_directory.starts_with(&game_root) {
            return Err(WinePlanError::WorkingDirectoryUnavailable);
        }

        let total_argument_bytes = request
            .arguments
            .iter()
            .map(|argument| argument.as_os_str().as_bytes().len())
            .sum::<usize>()
            .saturating_add(
                game_executable_path(&game_root, &request.game_executable_relative)?
                    .as_os_str()
                    .as_bytes()
                    .len(),
            );
        if request.arguments.len().saturating_add(1) > MAX_ARGUMENTS
            || request.arguments.iter().any(|argument| {
                argument.as_os_str().as_bytes().contains(&0)
                    || argument.as_os_str().as_bytes().len() > MAX_ARGUMENT_BYTES
            })
            || total_argument_bytes > MAX_TOTAL_ARGUMENT_BYTES
        {
            return Err(WinePlanError::InvalidArguments);
        }
        let mut native_arguments = Vec::with_capacity(request.arguments.len() + 1);
        native_arguments.push(game_executable.as_os_str().to_owned());
        native_arguments.extend(request.arguments);

        let mut plan = ManagedWineInvocationPlan {
            plan_id: String::new(),
            adapter_id: "managed_wine",
            binding: request.binding,
            expected_executable,
            native_arguments,
            working_directory,
            wine_prefix: prefix,
            environment_policy: WineEnvironmentPolicy::ClearThenDesktopSessionAllowlist,
            desktop_environment_allowlist: DESKTOP_ENVIRONMENT_ALLOWLIST.to_vec(),
            runtime_executable_identity,
            prefix_identity,
            game_executable_identity,
            process_lifetime: ProcessLifetimePolicy::DirectChildOnlyNoDaemonOwnership,
            approval_required: true,
            execution_supported: false,
            expected_effects: vec![
                "Wine may modify prefix registry and system files.",
                "Wine may create caches and wineserver-related processes.",
                "The game may access display, audio, input, network, and user data.",
            ],
        };
        plan.plan_id = invocation_plan_id(&plan);
        Ok(plan)
    }
}

fn invocation_plan_id(plan: &ManagedWineInvocationPlan) -> String {
    let mut hash = Sha256::new();
    hash_field(&mut hash, b"endfield-unix:p034:managed-wine-invocation:v1");
    hash_field(&mut hash, plan.adapter_id.as_bytes());
    let binding = &plan.binding;
    for value in [
        binding.runtime_installation_id.as_bytes(),
        binding.runtime_pin_id.as_bytes(),
        binding.runtime_reference_sha256.as_bytes(),
        binding.runtime_ownership_record_sha256.as_bytes(),
        binding.runtime_artifact_sha256.as_bytes(),
        binding.runtime_audit_report_sha256.as_bytes(),
        binding.prefix_id.as_bytes(),
        binding.prefix_runtime_pin_id.as_bytes(),
        binding.prefix_runtime_reference_sha256.as_bytes(),
        binding.game_installation_id.as_bytes(),
        plan.expected_executable.as_os_str().as_bytes(),
        plan.working_directory.as_os_str().as_bytes(),
        plan.wine_prefix.as_os_str().as_bytes(),
    ] {
        hash_field(&mut hash, value);
    }
    for value in [
        binding.runtime_pin_revision,
        binding.runtime_generation,
        binding.prefix_registry_revision,
        binding.prefix_generation,
        binding.prefix_device,
        binding.prefix_inode,
        binding.prefix_runtime_pin_revision,
        binding.game_registry_revision,
        binding.game_root_device,
        binding.game_root_inode,
        binding.game_executable_device,
        binding.game_executable_inode,
        plan.runtime_executable_identity.device,
        plan.runtime_executable_identity.inode,
        plan.runtime_executable_identity.size,
        plan.prefix_identity.device,
        plan.prefix_identity.inode,
        plan.prefix_identity.size,
        plan.game_executable_identity.device,
        plan.game_executable_identity.inode,
        plan.game_executable_identity.size,
        if plan.approval_required { 1 } else { 0 },
        if plan.execution_supported { 1 } else { 0 },
    ] {
        hash_field(&mut hash, &value.to_le_bytes());
    }
    for argument in &plan.native_arguments {
        hash_field(&mut hash, argument.as_os_str().as_bytes());
    }
    for effect in &plan.expected_effects {
        hash_field(&mut hash, effect.as_bytes());
    }
    hash_field(&mut hash, b"clear_then_desktop_session_allowlist");
    for name in &plan.desktop_environment_allowlist {
        hash_field(&mut hash, name.as_bytes());
    }
    hash_field(&mut hash, b"direct_child_only_no_daemon_ownership");
    format!("sha256:{:x}", hash.finalize())
}

fn hash_field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn validate_identifier(value: &str, required_prefix: &str) -> Result<(), WinePlanError> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        || !value.starts_with(required_prefix)
    {
        return Err(WinePlanError::InvalidIdentifier);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_absolute(path: &Path) -> Result<(), WinePlanError> {
    if !path.is_absolute()
        || path.as_os_str().len() > MAX_PATH_BYTES
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(WinePlanError::InvalidPath);
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), WinePlanError> {
    if path.as_os_str().is_empty()
        || path.as_os_str().len() > MAX_PATH_BYTES
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WinePlanError::InvalidPath);
    }
    Ok(())
}

fn checked_directory(path: &Path) -> Option<PathBuf> {
    validate_absolute(path).ok()?;
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    (canonical == path).then_some(canonical)
}

fn checked_executable(path: &Path) -> Option<FileIdentity> {
    validate_absolute(path).ok()?;
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o111 == 0
        || fs::canonicalize(path).ok()?.as_path() != path
    {
        return None;
    }
    Some(identity(&metadata))
}

fn game_executable_path(root: &Path, relative: &Path) -> Result<PathBuf, WinePlanError> {
    validate_relative(relative)?;
    Ok(root.join(relative))
}

fn identity(metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tempfile::tempdir;

    fn executable(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn binding(prefix: &Path, game_root: &Path, game_executable: &Path) -> InvocationBinding {
        let prefix_metadata = fs::metadata(prefix).unwrap();
        let root_metadata = fs::metadata(game_root).unwrap();
        let game_metadata = fs::metadata(game_executable).unwrap();
        InvocationBinding {
            runtime_installation_id: "wine-fixture".into(),
            runtime_pin_id: "pin-fixture".into(),
            runtime_pin_revision: 4,
            runtime_reference_sha256: format!("sha256:{}", "d".repeat(64)),
            runtime_generation: 1,
            runtime_ownership_record_sha256: "a".repeat(64),
            runtime_artifact_sha256: "b".repeat(64),
            runtime_audit_report_sha256: "c".repeat(64),
            prefix_id: "prefix-fixture".into(),
            prefix_registry_revision: 3,
            prefix_generation: 1,
            prefix_device: prefix_metadata.dev(),
            prefix_inode: prefix_metadata.ino(),
            prefix_runtime_pin_id: "pin-fixture".into(),
            prefix_runtime_pin_revision: 4,
            prefix_runtime_reference_sha256: format!("sha256:{}", "d".repeat(64)),
            game_installation_id: "endfield-main".into(),
            game_registry_revision: 8,
            game_root_device: root_metadata.dev(),
            game_root_inode: root_metadata.ino(),
            game_executable_device: game_metadata.dev(),
            game_executable_inode: game_metadata.ino(),
        }
    }

    #[test]
    fn wine_preview_keeps_unicode_and_shell_metacharacters_as_native_arguments() {
        let temporary = tempdir().unwrap();
        let runtime = temporary.path().join("runtimes/wine/wine-fixture");
        let game = temporary.path().join("game");
        let prefix = temporary.path().join("prefix");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        fs::create_dir(&game).unwrap();
        fs::create_dir(&prefix).unwrap();
        executable(&runtime.join("bin/wine"), b"fixture");
        executable(&game.join("Endfield Game.exe"), b"MZ");
        let args = vec![
            OsString::from("参数 ; $(touch should-not-exist)"),
            OsString::from("--literal=$HOME"),
        ];
        let plan = ManagedWineInvocationPlanBuilder::prepare(ManagedWineInvocationRequest {
            runtime_root: runtime.clone(),
            prefix: prefix.clone(),
            game_root: game.clone(),
            game_executable_relative: PathBuf::from("Endfield Game.exe"),
            working_directory: game.clone(),
            arguments: args.clone(),
            binding: binding(&prefix, &game, &game.join("Endfield Game.exe")),
        })
        .unwrap();

        assert_eq!(plan.expected_executable, runtime.join("bin/wine"));
        assert!(plan.plan_id.starts_with("sha256:"));
        assert_eq!(
            plan.native_arguments,
            vec![
                OsString::from(game.join("Endfield Game.exe")),
                args[0].clone(),
                args[1].clone(),
            ]
        );
        assert!(plan.approval_required);
        assert!(!plan.execution_supported);
        assert_eq!(
            plan.environment_policy,
            WineEnvironmentPolicy::ClearThenDesktopSessionAllowlist
        );
        assert!(!temporary.path().join("should-not-exist").exists());

        let repeated = ManagedWineInvocationPlanBuilder::prepare(ManagedWineInvocationRequest {
            runtime_root: runtime,
            prefix,
            game_root: game.clone(),
            game_executable_relative: PathBuf::from("Endfield Game.exe"),
            working_directory: game.clone(),
            arguments: args,
            binding: plan.binding.clone(),
        })
        .unwrap();
        assert_eq!(plan.plan_id, repeated.plan_id);
    }

    #[test]
    fn wine_preview_rejects_runtime_replacement_prefix_mismatch_and_symlink_executable() {
        let temporary = tempdir().unwrap();
        let runtime = temporary.path().join("runtimes/wine/wine-fixture");
        let game = temporary.path().join("game");
        let prefix = temporary.path().join("prefix");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        fs::create_dir(&game).unwrap();
        fs::create_dir(&prefix).unwrap();
        executable(&runtime.join("bin/wine"), b"fixture");
        executable(&game.join("Game.exe"), b"MZ");
        let base = binding(&prefix, &game, &game.join("Game.exe"));

        let mut changed_prefix = base.clone();
        changed_prefix.prefix_inode = changed_prefix.prefix_inode.wrapping_add(1);
        assert_eq!(
            ManagedWineInvocationPlanBuilder::prepare(ManagedWineInvocationRequest {
                runtime_root: runtime.clone(),
                prefix: prefix.clone(),
                game_root: game.clone(),
                game_executable_relative: PathBuf::from("Game.exe"),
                working_directory: game.clone(),
                arguments: vec![],
                binding: changed_prefix,
            })
            .unwrap_err(),
            WinePlanError::PrefixUnavailable
        );

        fs::remove_file(runtime.join("bin/wine")).unwrap();
        executable(&temporary.path().join("replacement-wine"), b"replacement");
        std::os::unix::fs::symlink(
            temporary.path().join("replacement-wine"),
            runtime.join("bin/wine"),
        )
        .unwrap();
        assert_eq!(
            ManagedWineInvocationPlanBuilder::prepare(ManagedWineInvocationRequest {
                runtime_root: runtime,
                prefix,
                game_root: game.clone(),
                game_executable_relative: PathBuf::from("Game.exe"),
                working_directory: game,
                arguments: vec![],
                binding: base,
            })
            .unwrap_err(),
            WinePlanError::RuntimeExecutableUnavailable
        );
    }
}
