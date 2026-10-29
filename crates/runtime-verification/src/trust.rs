use crate::{Error, Manifest, identifier, sha256, signing_input, timestamp};
use ed25519_dalek::{Signature, VerifyingKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    IndependentlyAuthenticatedPublisher,
    UserPinned,
    Unverified,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub provider: String,
    pub project: String,
    pub channel: String,
    pub family: String,
    pub lineage: String,
}
impl Scope {
    pub fn matches(&self, m: &Manifest) -> bool {
        self.provider == m.provider_id
            && self.project == m.project_id
            && self.channel == m.channel
            && self.family == m.runtime_family
            && self.lineage == m.runtime_lineage
    }
}
#[derive(Clone, Debug)]
pub struct TrustAnchor {
    pub key_id: String,
    pub public_key: [u8; 32],
    pub scope: Scope,
    pub provenance: Provenance,
    pub revoked: bool,
    pub valid_from: Option<i64>,
    pub valid_until: Option<i64>,
}
/// Caller attests these checkpoints were authenticated and retained securely.
/// Scope is provider/project/channel/family/lineage; versions are global in that scope.
#[derive(Clone, Debug)]
pub struct TrustedCheckpoint {
    pub scope: Scope,
    pub manifest_id: String,
    pub version: u64,
    pub content_sha256: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollbackStatus {
    NoCheckpoint,
    Checked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpirationStatus {
    Checked,
    TrustedTimeUnavailable,
}
#[derive(Debug)]
pub struct VerifiedManifest {
    pub(crate) manifest: Manifest,
    pub(crate) content_sha256: String,
    pub(crate) provenance: Provenance,
    pub(crate) time: Option<i64>,
    pub(crate) rollback: RollbackStatus,
}
impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn provenance(&self) -> Provenance {
        self.provenance
    }
    pub fn rollback(&self) -> RollbackStatus {
        self.rollback
    }
    pub fn expiration(&self) -> ExpirationStatus {
        if self.time.is_some() {
            ExpirationStatus::Checked
        } else {
            ExpirationStatus::TrustedTimeUnavailable
        }
    }
    pub fn checkpoint(&self) -> Result<TrustedCheckpoint, Error> {
        if self.provenance != Provenance::IndependentlyAuthenticatedPublisher || self.time.is_none()
        {
            return Err(Error::UntrustedKey);
        }
        let m = &self.manifest;
        Ok(TrustedCheckpoint {
            scope: Scope {
                provider: m.provider_id.clone(),
                project: m.project_id.clone(),
                channel: m.channel.clone(),
                family: m.runtime_family.clone(),
                lineage: m.runtime_lineage.clone(),
            },
            manifest_id: m.manifest_id.clone(),
            version: m.manifest_version,
            content_sha256: self.content_sha256.clone(),
        })
    }
}
pub fn verify_manifest(
    bytes: &[u8],
    signature: &[u8],
    anchors: &[TrustAnchor],
    now: Option<i64>,
    checkpoint: Option<&TrustedCheckpoint>,
) -> Result<VerifiedManifest, Error> {
    let m = Manifest::parse(bytes)?;
    if anchors.len() > 32 || anchors.iter().any(|a| !identifier(&a.key_id)) {
        return Err(Error::Scope);
    }
    let mut ids = std::collections::BTreeSet::new();
    if anchors.iter().any(|a| !ids.insert(&a.key_id)) {
        return Err(Error::Scope);
    }
    let a = anchors
        .iter()
        .find(|a| a.key_id == m.key_id)
        .ok_or(Error::UnknownKey)?;
    if a.revoked {
        return Err(Error::RevokedKey);
    }
    if a.provenance == Provenance::Unverified {
        return Err(Error::UntrustedKey);
    }
    if !a.scope.matches(&m) {
        return Err(Error::Scope);
    }
    let key = VerifyingKey::from_bytes(&a.public_key).map_err(|_| Error::Signature)?;
    if key.is_weak() {
        return Err(Error::Signature);
    }
    let sig = Signature::from_slice(signature).map_err(|_| Error::Signature)?;
    key.verify_strict(&signing_input(bytes), &sig)
        .map_err(|_| Error::Signature)?;
    match now {
        Some(t) => {
            if t < 0 || a.valid_from.is_some_and(|v| t < v) || a.valid_until.is_some_and(|v| t >= v)
            {
                return Err(Error::Expired);
            }
            if timestamp(&m.issued_at)? > t {
                return Err(Error::FutureIssued);
            }
            if timestamp(&m.expires_at)? <= t {
                return Err(Error::Expired);
            }
        }
        None if a.valid_from.is_some() || a.valid_until.is_some() => {
            return Err(Error::TimeUnknown);
        }
        None => (),
    }
    let content_sha256 = sha256(bytes);
    let rollback = if let Some(c) = checkpoint {
        if !c.scope.matches(&m)
            || c.version == 0
            || c.version > crate::MAX_MANIFEST_VERSION
            || !identifier(&c.manifest_id)
            || c.content_sha256.len() != 64
            || !c.content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Scope);
        }
        if m.manifest_version < c.version {
            return Err(Error::Rollback);
        }
        if m.manifest_version == c.version
            && (c.content_sha256 != content_sha256 || c.manifest_id != m.manifest_id)
        {
            return Err(Error::CheckpointConflict);
        }
        RollbackStatus::Checked
    } else {
        RollbackStatus::NoCheckpoint
    };
    Ok(VerifiedManifest {
        manifest: m,
        content_sha256,
        provenance: a.provenance,
        time: now,
        rollback,
    })
}
