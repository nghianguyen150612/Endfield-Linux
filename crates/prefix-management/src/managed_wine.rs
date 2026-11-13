//! Read-only P025/P026/P027 validation shared by previews and the coordinator.
//! A verified top-level ELF is not authorization for its loader/helper closure.
use endfield_unix_game_launch::linux_executor::{BoundExecutable, ExpectedExecutableIdentity};
use endfield_unix_runtime_selection as pins;
use std::path::Path;

/// Produced only by full managed installation and exact pin validation.
/// No public descriptor accessor: this evidence cannot authorize spawning.
pub struct VerifiedManagedWineExecutable {
    pub(crate) executable: BoundExecutable,
    digest: String,
}

impl VerifiedManagedWineExecutable {
    pub fn sha256(&self) -> &str {
        &self.digest
    }
    pub fn dynamic_linker_present(&self) -> bool {
        self.executable.dynamic_linker().is_some()
    }
}

pub fn verify_managed_wine_executable(
    paths: &endfield_unix_storage::AppPaths,
    pin_id: &str,
    pin_revision: u64,
    reference: &pins::PinnedRuntimeReference,
    expected: ExpectedExecutableIdentity,
) -> Result<VerifiedManagedWineExecutable, &'static str> {
    use endfield_unix_runtime_audit::{Completeness, EntryKind, FindingKind, TrustClassification};
    use std::os::unix::fs::MetadataExt;

    let registry = pins::PinStore::for_app(paths)
        .load_read_only()
        .map_err(|_| "P027 runtime pin registry could not be revalidated")?;
    if registry.revision != pin_revision {
        return Err("P027 runtime pin revision changed during executable validation");
    }
    let pin = registry
        .pins
        .iter()
        .find(|pin| pin.pin_id == pin_id)
        .ok_or("exact P027 runtime pin disappeared")?;
    if pin.adapter_preference != Some(pins::AdapterPreference::ManagedWine)
        || &pin.runtime != reference
    {
        return Err("P027 runtime pin identity or adapter changed");
    }
    reference
        .validate()
        .map_err(|_| "invalid P027 runtime reference")?;
    let identity = endfield_unix_runtime_install::pin_identity(
        paths,
        match reference {
            pins::PinnedRuntimeReference::Managed(reference) => &reference.installation_id,
            _ => return Err("selected runtime is not managed Wine"),
        },
    )
    .map_err(|_| "P025/P026 runtime revalidation failed")?;

    let pins::PinnedRuntimeReference::Managed(reference) = reference else {
        return Err("selected runtime is not a managed Wine installation");
    };
    if identity.installation_id != reference.installation_id
        || reference.trust != pins::Provenance::IndependentlyAuthenticatedPublisher
        || identity.provider != reference.provider
        || identity.project != reference.project
        || identity.family != reference.family
        || identity.lineage != reference.lineage
        || identity.build != reference.build
        || identity.family != "wine"
        || identity.lineage != "upstream-wine"
        || identity.build != "wine-10.0"
        || identity.generation != reference.generation
        || identity.ownership_record_sha256 != reference.ownership_record_sha256
        || identity.artifact_sha256 != reference.artifact_sha256
        || identity.manifest_sha256 != reference.manifest_sha256
        || identity.trust != endfield_unix_runtime_install::TrustClassification::IndependentlyAuthenticatedPublisher
        || identity.audit.completeness != Completeness::Complete
        || identity
            .audit
            .findings
            .iter()
            .any(|finding| finding.kind != FindingKind::Match)
    {
        return Err("P025 ownership or P026 full runtime integrity evidence changed");
    }

    let runtime_root = paths
        .data_dir()
        .join("runtimes")
        .join("wine")
        .join(&reference.installation_id);
    let root_metadata = std::fs::symlink_metadata(&runtime_root)
        .map_err(|_| "managed Wine runtime root is unavailable")?;
    // SAFETY: geteuid has no preconditions.
    let expected_uid = unsafe { libc::geteuid() };
    if !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || root_metadata.uid() != expected_uid
    {
        return Err("managed Wine runtime root owner or directory type is invalid");
    }
    let root = endfield_unix_game_launch::linux_executor::TrustedRoot::open(
        &runtime_root,
        endfield_unix_game_launch::linux_executor::DirectoryIdentity {
            device: root_metadata.dev(),
            inode: root_metadata.ino(),
            owner_uid: root_metadata.uid(),
        },
        expected_uid,
    )
    .map_err(|_| "managed Wine runtime root failed no-follow identity validation")?;
    let baseline = endfield_unix_runtime_audit::load_baseline(&runtime_root)
        .map_err(|_| "P026 runtime baseline could not be reloaded")?;
    let expected_trust = match identity.trust {
        endfield_unix_runtime_install::TrustClassification::IndependentlyAuthenticatedPublisher => {
            TrustClassification::IndependentlyAuthenticatedPublisher
        }
        endfield_unix_runtime_install::TrustClassification::UserPinnedLimitedTrust => {
            TrustClassification::UserPinnedLimitedTrust
        }
    };
    if baseline.installation_id != identity.installation_id
        || baseline.generation != identity.generation
        || baseline.family != identity.family
        || baseline.build != identity.build
        || baseline.artifact_sha256 != identity.artifact_sha256
        || baseline.manifest_sha256 != identity.manifest_sha256
        || baseline.trust != expected_trust
    {
        return Err("P026 baseline does not bind the selected Wine generation");
    }
    let entry = baseline
        .entries
        .iter()
        .find(|entry| entry.path == "bin/wine")
        .ok_or("P026 baseline has no exact bin/wine entry")?;
    let (size, digest) = match &entry.kind {
        EntryKind::File {
            size,
            sha256,
            executable: true,
            setid: false,
        } => (*size, sha256.as_str()),
        _ => return Err("P026 bin/wine entry is not an approved executable regular file"),
    };
    let executable = endfield_unix_game_launch::linux_executor::BoundExecutable::open_matching(
        &root,
        Path::new("bin/wine"),
        expected_uid,
        endfield_unix_game_launch::linux_executor::ExpectedExecutableIdentity {
            device: expected.device,
            inode: expected.inode,
            size: expected.size,
        },
        digest,
    )
    .map_err(|_| "opened bin/wine ELF identity or bytes differ from P026 baseline")?;
    if size != executable.identity().size {
        return Err("opened bin/wine size differs from the P026 baseline");
    }

    let fresh = endfield_unix_runtime_install::pin_identity(paths, &reference.installation_id)
        .map_err(|_| "P025/P026 final runtime revalidation failed")?;
    if fresh.ownership_record_sha256 != identity.ownership_record_sha256
        || fresh.generation != identity.generation
        || fresh.audit.completeness != Completeness::Complete
        || fresh
            .audit
            .findings
            .iter()
            .any(|finding| finding.kind != FindingKind::Match)
    {
        return Err("P025/P026 runtime identity changed during executable validation");
    }
    let final_registry = pins::PinStore::for_app(paths)
        .load_read_only()
        .map_err(|_| "P027 final pin revalidation failed")?;
    if final_registry.revision != pin_revision
        || !final_registry.pins.iter().any(|candidate| {
            candidate.pin_id == pin_id
                && candidate.runtime == pins::PinnedRuntimeReference::Managed(reference.clone())
                && candidate.adapter_preference == Some(pins::AdapterPreference::ManagedWine)
        })
    {
        return Err("P027 runtime pin changed during executable validation");
    }
    Ok(VerifiedManagedWineExecutable {
        executable,
        digest: digest.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use endfield_unix_storage::AppPaths;

    #[test]
    fn forged_provenance_and_stale_pins_cannot_produce_descriptor_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_base_dirs(
            directory.path().join("config"),
            directory.path().join("data"),
            directory.path().join("cache"),
        )
        .unwrap();
        let reference = pins::PinnedRuntimeReference::Managed(pins::ManagedRuntimeReference {
            installation_id: "wine-forged".into(),
            family: "wine".into(),
            lineage: "upstream-wine".into(),
            provider: "synthetic-tests".into(),
            project: "synthetic-tests".into(),
            build: "wine-10.0".into(),
            artifact_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            generation: 1,
            ownership_record_sha256: "c".repeat(64),
            trust: pins::Provenance::IndependentlyAuthenticatedPublisher,
        });
        let expected = ExpectedExecutableIdentity {
            device: 1,
            inode: 1,
            size: 1,
        };
        assert!(verify_managed_wine_executable(&paths, "forged", 0, &reference, expected).is_err());
        assert!(!paths.data_dir().exists());
        let store = pins::PinStore::for_app(&paths);
        let pin = pins::RuntimePin::new(
            "forged",
            pins::RuntimePinScope::Machine {
                machine: pins::MachineId::from_digest("d".repeat(64)).unwrap(),
            },
            reference.clone(),
            Some(pins::AdapterPreference::ManagedWine),
            1,
        )
        .unwrap();
        store.set(pin, 0, false).unwrap();
        let before = store.load_read_only().unwrap();
        assert!(matches!(
            verify_managed_wine_executable(&paths, "forged", 0, &reference, expected),
            Err("P027 runtime pin revision changed during executable validation")
        ));
        // Even a matching persisted pin and authenticated-publisher claim need
        // independently loaded installation and artifact evidence.
        assert!(matches!(
            verify_managed_wine_executable(&paths, "forged", 1, &reference, expected),
            Err("P025/P026 runtime revalidation failed")
        ));
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&store.load_read_only().unwrap()).unwrap()
        );
        assert!(!paths.data_dir().join("runtimes").exists());
    }
}
