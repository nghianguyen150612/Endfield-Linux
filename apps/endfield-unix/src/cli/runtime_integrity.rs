use endfield_unix_runtime_verification::*;
use std::{
    io::{self, Write},
    path::PathBuf,
};
#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
pub struct Options {
    #[arg(long, value_parser = super::parser::absolute_path)]
    pub manifest: PathBuf,
    #[arg(long, value_parser = super::parser::absolute_path)]
    pub signature: PathBuf,
    #[arg(long, value_parser = super::parser::absolute_path)]
    pub artifact: PathBuf,
    #[arg(long, value_parser = super::parser::absolute_path)]
    pub trust_key: PathBuf,
    #[arg(long)]
    pub provider: String,
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub channel: String,
    #[arg(long)]
    pub family: String,
    #[arg(long)]
    pub lineage: String,
    #[arg(long)]
    pub build: String,
    #[arg(long)]
    pub artifact_id: String,
    #[arg(long)]
    pub key_id: String,
}

pub fn run(o: Options, out: &mut impl Write) -> io::Result<u8> {
    let result = (|| {
        let bytes = read_bounded(&o.manifest, MAX_MANIFEST_BYTES)?;
        let sig = read_bounded(&o.signature, 64).map_err(signature_size)?;
        let key: [u8; 32] = read_bounded(&o.trust_key, 32)
            .map_err(signature_size)?
            .try_into()
            .map_err(|_| Error::Signature)?;
        let anchor = TrustAnchor {
            key_id: o.key_id,
            public_key: key,
            scope: Scope {
                provider: o.provider,
                project: o.project,
                channel: o.channel,
                family: o.family,
                lineage: o.lineage,
            },
            provenance: Provenance::UserPinned,
            revoked: false,
            valid_from: None,
            valid_until: None,
        };
        // Explicit policy: trust the host clock for this read-only diagnostic.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|t| i64::try_from(t.as_secs()).ok());
        let metadata = verify_manifest(&bytes, &sig, &[anchor], now, None)?;
        let filename = o
            .artifact
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::Identity)?;
        verify_artifact(ArtifactInput {
            artifact_id: &o.artifact_id,
            filename,
            build: &o.build,
            file: open_artifact(&o.artifact)?,
            metadata: &metadata,
        })
    })();
    match result {
        Ok(r) => {
            writeln!(
                out,
                "Manifest: {} (schema v{}, version {})",
                r.manifest_id, r.schema_version, r.manifest_version
            )?;
            writeln!(
                out,
                "Provider: {}\nProject: {}\nBuild: {}\nChannel: {}\nKey: {}",
                r.source.identity().provider_id(),
                r.source
                    .identity()
                    .project_id()
                    .map_or("unknown", |p| p.as_str()),
                r.source
                    .identity()
                    .release_or_build_id()
                    .map_or("unknown", |b| b.as_str()),
                r.channel,
                r.key_id
            )?;
            writeln!(
                out,
                "Runtime artifact verification\nSignature: valid\nKey scope: matched\nPublisher authenticity: user-pinned only\nManifest expiration: {:?}\nRollback checkpoint: {:?}\nSize: matches ({} bytes)\nSHA-256: matches\nSource-integrity status: local match\nOfficial publisher authenticity: UNVERIFIED\nFreshness: not fully established\nGame compatibility: not evaluated\nInstallation performed: no",
                r.expiration, r.rollback, r.size
            )?;
            Ok(3)
        }
        Err(e) => {
            writeln!(out, "Runtime artifact verification: {e}")?;
            Ok(match e {
                Error::UnsupportedAlgorithm => 8,
                Error::Signature | Error::UnknownKey | Error::RevokedKey | Error::UntrustedKey => 4,
                Error::DigestMismatch | Error::SizeMismatch => 5,
                Error::FileUnavailable
                | Error::PermissionDenied
                | Error::FileType
                | Error::ChangedDuringRead
                | Error::ReadFailure => 7,
                _ => 6,
            })
        }
    }
}

fn signature_size(e: Error) -> Error {
    if e == Error::TooLarge {
        Error::Signature
    } else {
        e
    }
}
