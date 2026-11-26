//! Sealed candidate backing bytes and fail-closed runtime-view preparation.
//! Ordinary read-only binds do not freeze mutable backing files. No production
//! view is issued without a trusted mount/image authority, absent in this version.
use endfield_unix_runtime_verification::sha256;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::fs::FileExt,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewScope {
    pub artifact_sha256: String,
    pub inventory_sha256: String,
    pub profile_sha256: String,
    pub generation: u64,
}

/// Verified image bytes bound to one P025 candidate scope. This is not a mount.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeImageScope {
    pub artifact_sha256: String,
    pub image_sha256: String,
    pub inventory_sha256: String,
    pub profile_sha256: String,
    pub generation: u64,
    pub image_format: String,
    pub provider_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewBlocker {
    InvalidScope,
    ChangedArtifact,
    SealUnavailable,
    NamespaceMountUnavailable,
    TrustedImageMountAuthorityUnavailable,
    ChangedScope,
    Io,
    CleanupFailed,
}
#[derive(Debug, Serialize)]
pub struct ViewCapabilities {
    pub sealed_memfd_supported: bool,
    pub private_tmpfs_mount_supported: bool,
    pub execution_blocker: ViewBlocker,
    pub guarantees: &'static str,
}

/// Backing object only. Not an execution capability and never exposed as a path.
pub struct PreparedBacking {
    file: File,
    scope: ViewScope,
    size: u64,
}
/// Sealed filesystem-image backing. No mounted tree or process lifecycle is issued.
pub struct PreparedRuntimeImage {
    backing: PreparedBacking,
    scope: RuntimeImageScope,
}
const SEALS: i32 = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
impl PreparedBacking {
    pub fn prepare(bytes: &[u8], scope: ViewScope) -> Result<Self, ViewBlocker> {
        let valid = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if scope.generation == 0
            || !valid(&scope.artifact_sha256)
            || !valid(&scope.inventory_sha256)
            || !valid(&scope.profile_sha256)
            || bytes.len() > 512_000_000
        {
            return Err(ViewBlocker::InvalidScope);
        }
        if sha256(bytes) != scope.artifact_sha256 {
            return Err(ViewBlocker::ChangedArtifact);
        }
        // SAFETY: static NUL-terminated name, valid flags; a successful fd is owned once.
        let fd = unsafe {
            libc::memfd_create(
                c"endfield-candidate-backing".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if fd < 0 {
            return Err(ViewBlocker::SealUnavailable);
        }
        // SAFETY: successful memfd_create returned a fresh owned descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(bytes).map_err(|_| ViewBlocker::Io)?;
        // SAFETY: owned open memfd, supported seal bits.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, SEALS) } != 0 {
            return Err(ViewBlocker::SealUnavailable);
        }
        let value = Self {
            file,
            scope,
            size: bytes.len() as u64,
        };
        value.revalidate(&value.scope)?;
        Ok(value)
    }
    pub fn revalidate(&self, expected: &ViewScope) -> Result<(), ViewBlocker> {
        if &self.scope != expected {
            return Err(ViewBlocker::ChangedScope);
        }
        // SAFETY: querying seals on a live descriptor.
        let seals = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_GET_SEALS) };
        if seals < 0 || seals & SEALS != SEALS {
            return Err(ViewBlocker::SealUnavailable);
        }
        if self.file.metadata().map_err(|_| ViewBlocker::Io)?.len() != self.size {
            return Err(ViewBlocker::ChangedArtifact);
        }
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024];
        let mut offset = 0;
        while offset < self.size {
            let remaining = (self.size - offset).min(buffer.len() as u64) as usize;
            let count = self
                .file
                .read_at(&mut buffer[..remaining], offset)
                .map_err(|_| ViewBlocker::Io)?;
            if count == 0 {
                return Err(ViewBlocker::ChangedArtifact);
            }
            digest.update(&buffer[..count]);
            offset += count as u64;
        }
        if format!("{:x}", digest.finalize()) != self.scope.artifact_sha256 {
            return Err(ViewBlocker::ChangedArtifact);
        }
        Ok(())
    }
    pub fn close(self) -> Result<(), ViewBlocker> {
        close_owned(self.file.into_raw_fd())
    }

    pub fn execution_blocker(&self) -> ViewBlocker {
        ViewBlocker::TrustedImageMountAuthorityUnavailable
    }
}

impl PreparedRuntimeImage {
    pub fn prepare(bytes: &[u8], scope: RuntimeImageScope) -> Result<Self, ViewBlocker> {
        let valid = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if scope.generation == 0
            || !valid(&scope.artifact_sha256)
            || !valid(&scope.image_sha256)
            || !valid(&scope.inventory_sha256)
            || !valid(&scope.profile_sha256)
            || scope.image_format != "squashfs"
            || scope.provider_id != "squashfuse"
        {
            return Err(ViewBlocker::InvalidScope);
        }
        if sha256(bytes) != scope.image_sha256 {
            return Err(ViewBlocker::ChangedArtifact);
        }
        let backing_scope = ViewScope {
            artifact_sha256: scope.image_sha256.clone(),
            inventory_sha256: scope.inventory_sha256.clone(),
            profile_sha256: scope.profile_sha256.clone(),
            generation: scope.generation,
        };
        Ok(Self {
            backing: PreparedBacking::prepare(bytes, backing_scope)?,
            scope,
        })
    }

    pub fn revalidate(&self, expected: &RuntimeImageScope) -> Result<(), ViewBlocker> {
        if &self.scope != expected {
            return Err(ViewBlocker::ChangedScope);
        }
        self.backing.revalidate(&ViewScope {
            artifact_sha256: expected.image_sha256.clone(),
            inventory_sha256: expected.inventory_sha256.clone(),
            profile_sha256: expected.profile_sha256.clone(),
            generation: expected.generation,
        })
    }

    pub fn scope(&self) -> &RuntimeImageScope {
        &self.scope
    }

    pub fn execution_blocker(&self) -> ViewBlocker {
        ViewBlocker::TrustedImageMountAuthorityUnavailable
    }

    pub fn close(self) -> Result<(), ViewBlocker> {
        self.backing.close()
    }
}

fn close_owned(fd: i32) -> Result<(), ViewBlocker> {
    // SAFETY: production passes the uniquely owned descriptor from into_raw_fd.
    // Do not retry EINTR: Linux may already have released the descriptor.
    if unsafe { libc::close(fd) } == 0 {
        Ok(())
    } else {
        Err(ViewBlocker::CleanupFailed)
    }
}

/// Probe only in a child namespace. Never mounts over the caller's namespace.
/// Success does not qualify a view: read-only bind aliases remain mutable.
pub fn inspect_capabilities(
    probe_directory: &std::path::Path,
) -> Result<ViewCapabilities, ViewBlocker> {
    use std::{
        os::unix::fs::MetadataExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let metadata = std::fs::symlink_metadata(probe_directory).map_err(|_| ViewBlocker::Io)?;
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ViewBlocker::InvalidScope);
    }
    // Only root-owned non-writable distro utilities may run this bounded probe.
    for tool in ["/usr/bin/unshare", "/usr/bin/mount"] {
        let metadata =
            std::fs::metadata(tool).map_err(|_| ViewBlocker::NamespaceMountUnavailable)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || !metadata.is_file() {
            return Err(ViewBlocker::NamespaceMountUnavailable);
        }
    }
    let mut child = Command::new("/usr/bin/unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
            "/usr/bin/mount",
            "-t",
            "tmpfs",
            "-o",
            "ro,nosuid,nodev,noexec",
            "none",
        ])
        .arg(probe_directory)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ViewBlocker::NamespaceMountUnavailable)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mounted = loop {
        if let Some(status) = child.try_wait().map_err(|_| ViewBlocker::Io)? {
            break status.success();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let scope = ViewScope {
        artifact_sha256: sha256(b"probe"),
        inventory_sha256: "0".repeat(64),
        profile_sha256: "0".repeat(64),
        generation: 1,
    };
    let sealed = PreparedBacking::prepare(b"probe", scope).is_ok();
    Ok(ViewCapabilities {
        sealed_memfd_supported: sealed,
        private_tmpfs_mount_supported: mounted,
        execution_blocker: if !sealed {
            ViewBlocker::SealUnavailable
        } else if !mounted {
            ViewBlocker::NamespaceMountUnavailable
        } else {
            ViewBlocker::TrustedImageMountAuthorityUnavailable
        },
        guarantees: "Seals protect backing bytes against write/truncate through aliases. No mounted Wine view, helper identity, host-library closure or protection against ptrace/root is claimed.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> ViewScope {
        ViewScope {
            artifact_sha256: sha256(b"fixture"),
            inventory_sha256: "1".repeat(64),
            profile_sha256: "2".repeat(64),
            generation: 1,
        }
    }
    #[test]
    fn seals_reject_alias_writes_truncation_and_changed_scope() {
        let backing = PreparedBacking::prepare(b"fixture", scope()).unwrap();
        let alias = backing.file.try_clone().unwrap();
        // SAFETY: valid descriptor and bounded pointer, offset zero.
        assert_eq!(
            unsafe { libc::pwrite(alias.as_raw_fd(), b"attack".as_ptr().cast(), 6, 0) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        );
        assert!(alias.set_len(0).is_err());
        for change in 0..4 {
            let mut altered = scope();
            match change {
                0 => altered.generation += 1,
                1 => altered.artifact_sha256 = "0".repeat(64),
                2 => altered.inventory_sha256 = "0".repeat(64),
                _ => altered.profile_sha256 = "0".repeat(64),
            }
            assert_eq!(backing.revalidate(&altered), Err(ViewBlocker::ChangedScope));
        }
        backing.revalidate(&scope()).unwrap();
        assert_eq!(
            backing.execution_blocker(),
            ViewBlocker::TrustedImageMountAuthorityUnavailable
        );
        use std::os::unix::fs::MetadataExt;
        let fd = backing.file.as_raw_fd();
        let identity = backing.file.metadata().unwrap();
        drop(alias);
        backing.close().unwrap();
        assert_eq!(close_owned(-1), Err(ViewBlocker::CleanupFailed));
        // Other parallel fixtures may reuse the number; the original backing
        // object must no longer occupy this descriptor.
        match std::fs::metadata(format!("/proc/self/fd/{fd}")) {
            Ok(current) => assert_ne!(
                (current.dev(), current.ino()),
                (identity.dev(), identity.ino())
            ),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
    }
    #[test]
    fn changed_bytes_and_invalid_generation_never_prepare() {
        assert!(matches!(
            PreparedBacking::prepare(b"changed", scope()),
            Err(ViewBlocker::ChangedArtifact)
        ));
        let mut bad = scope();
        bad.generation = 0;
        assert!(matches!(
            PreparedBacking::prepare(b"fixture", bad),
            Err(ViewBlocker::InvalidScope)
        ));
    }
    #[test]
    fn image_identity_is_separate_from_candidate_and_does_not_claim_a_mount() {
        let bytes = b"verified fixture squashfs bytes";
        let scope = RuntimeImageScope {
            artifact_sha256: sha256(b"source artifact"),
            image_sha256: sha256(bytes),
            inventory_sha256: "1".repeat(64),
            profile_sha256: "2".repeat(64),
            generation: 7,
            image_format: "squashfs".into(),
            provider_id: "squashfuse".into(),
        };
        let image = PreparedRuntimeImage::prepare(bytes, scope.clone()).unwrap();
        image.revalidate(&scope).unwrap();
        assert_eq!(image.scope(), &scope);
        assert_eq!(
            image.execution_blocker(),
            ViewBlocker::TrustedImageMountAuthorityUnavailable
        );
        let mut changed = scope.clone();
        changed.generation += 1;
        assert_eq!(image.revalidate(&changed), Err(ViewBlocker::ChangedScope));
        let mut changed_inventory = scope.clone();
        changed_inventory.inventory_sha256 = "3".repeat(64);
        assert_eq!(
            image.revalidate(&changed_inventory),
            Err(ViewBlocker::ChangedScope)
        );
        let mut changed_profile = scope.clone();
        changed_profile.profile_sha256 = "4".repeat(64);
        assert_eq!(
            image.revalidate(&changed_profile),
            Err(ViewBlocker::ChangedScope)
        );
        let mut changed_provider = scope.clone();
        changed_provider.provider_id = "other-provider".into();
        assert_eq!(
            image.revalidate(&changed_provider),
            Err(ViewBlocker::ChangedScope)
        );
        assert!(matches!(
            PreparedRuntimeImage::prepare(b"changed", scope),
            Err(ViewBlocker::ChangedArtifact)
        ));
        image.close().unwrap();
    }
    #[test]
    fn private_mount_probe_cannot_authorize_or_change_parent_view() {
        let directory = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let sentinel = directory.path().join("sentinel");
        std::fs::write(&sentinel, b"unchanged").unwrap();
        let before = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
        match inspect_capabilities(directory.path()) {
            Ok(capabilities) => assert!(matches!(
                capabilities.execution_blocker,
                ViewBlocker::NamespaceMountUnavailable
                    | ViewBlocker::TrustedImageMountAuthorityUnavailable
                    | ViewBlocker::SealUnavailable
            )),
            Err(error) => assert_eq!(error, ViewBlocker::NamespaceMountUnavailable),
        }
        assert_eq!(std::fs::read(sentinel).unwrap(), b"unchanged");
        assert_eq!(
            std::fs::read_to_string("/proc/self/mountinfo").unwrap(),
            before
        );
    }
    #[test]
    fn view_creation_failure_is_fail_closed() {
        use std::os::unix::fs::PermissionsExt;
        let public = tempfile::tempdir().unwrap();
        std::fs::set_permissions(public.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            inspect_capabilities(public.path()),
            Err(ViewBlocker::InvalidScope)
        ));
        assert!(matches!(
            inspect_capabilities(std::path::Path::new("/nonexistent-p034-runtime-view")),
            Err(ViewBlocker::Io)
        ));
    }
}

#[cfg(test)]
mod actual_candidate_backing {
    use super::*;
    #[test]
    #[ignore = "explicit acquired artifact sealing observation; no runtime execution"]
    fn seal_actual_candidate_without_execution() {
        let path = std::env::var("ENDFIELD_SEALED_CANDIDATE").unwrap();
        let report_path = std::env::var("ENDFIELD_SEALED_CANDIDATE_REPORT").unwrap();
        let raw = include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-profile.json");
        let profile: crate::wine_candidate::CandidateProfile = serde_json::from_slice(raw).unwrap();
        let scope = ViewScope {
            artifact_sha256: profile.derived_artifact_sha256.clone().unwrap(),
            inventory_sha256: profile.inventory_sha256.clone(),
            profile_sha256: sha256(raw),
            generation: 1,
        };
        let inventory: Vec<endfield_unix_runtime_audit::Entry> =
            serde_json::from_slice(include_bytes!(
                "../../../docs/artifact-evidence/winehq-dualarch-transformed-inventory.json"
            ))
            .unwrap();
        let transformation =
            include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-packaging.json");
        profile
            .validate_derived_evidence(
                &inventory,
                transformation,
                &scope.artifact_sha256,
                scope.generation,
            )
            .unwrap();
        profile
            .validate_source_evidence(
                include_bytes!(
                    "../../../docs/artifact-evidence/winehq-dualarch-source-inventory.json"
                ),
                transformation,
            )
            .unwrap();
        let bytes = std::fs::read(path).unwrap();
        let prepared = PreparedBacking::prepare(&bytes, scope.clone()).unwrap();
        prepared.revalidate(&scope).unwrap();
        let probe = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(probe.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let capabilities = inspect_capabilities(probe.path()).unwrap();
        prepared.close().unwrap();
        std::fs::write(report_path,serde_json::to_vec_pretty(&serde_json::json!({"scope":scope,"backing":"SEALED_AND_DIGEST_REVALIDATED","lifecycle":"BACKING_CLOSED_NO_MOUNT_CREATED","capabilities":capabilities,"mounted_runtime_view":"UNAVAILABLE","execution_authorized":false,"authentic_execution":"NOT_RUN"})).unwrap()).unwrap();
    }
}
