use crate::Error;
use serde::Deserialize;
use std::collections::BTreeSet;
pub const MAX_MANIFEST_BYTES: usize = 262144;
pub const MAX_ARTIFACTS: usize = 128;
pub const MAX_MANIFEST_VERSION: u64 = 9_223_372_036_854_775_807;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const DOMAIN: &[u8] = b"ENDFIELD-UNIX-RUNTIME-MANIFEST-V1\0";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub manifest_id: String,
    pub manifest_version: u64,
    pub provider_id: String,
    pub project_id: String,
    pub channel: String,
    pub runtime_family: String,
    pub runtime_lineage: String,
    pub runtime_build: String,
    pub key_id: String,
    pub signature_algorithm: String,
    pub issued_at: String,
    pub expires_at: String,
    pub artifacts: Vec<Artifact>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub artifact_id: String,
    pub filename: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub sha256: String,
}
pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub fn timestamp(s: &str) -> Result<i64, Error> {
    if s.len() != 20 || !s.ends_with('Z') {
        return Err(Error::Schema);
    }
    let t = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| Error::Schema)?;
    if t.timestamp() < 0 {
        return Err(Error::Schema);
    }
    Ok(t.timestamp())
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(Error::TooLarge);
        }
        // Serde's typed structs reject duplicate fields at both levels; unknown fields
        // are rejected as well. Default recursion limit stays enabled.
        let m: Self = serde_json::from_slice(bytes).map_err(|_| Error::Schema)?;
        if m.schema_version != 1
            || m.manifest_version == 0
            || m.manifest_version > MAX_MANIFEST_VERSION
            || ![
                &m.manifest_id,
                &m.provider_id,
                &m.project_id,
                &m.channel,
                &m.runtime_build,
                &m.key_id,
                &m.runtime_lineage,
            ]
            .iter()
            .all(|s| identifier(s))
            || !matches!(m.runtime_family.as_str(), "wine" | "proton")
            || !matches!(
                (m.runtime_family.as_str(), m.runtime_lineage.as_str()),
                ("wine", "upstream-wine" | "wine-staging" | "custom")
                    | (
                        "proton",
                        "valve-proton" | "ge-proton" | "dw-proton" | "custom"
                    )
            )
            || m.artifacts.is_empty()
            || m.artifacts.len() > MAX_ARTIFACTS
            || timestamp(&m.issued_at)? >= timestamp(&m.expires_at)?
        {
            return Err(Error::Schema);
        }
        if m.signature_algorithm != "ed25519" {
            return Err(Error::UnsupportedAlgorithm);
        }
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for a in &m.artifacts {
            if !identifier(&a.artifact_id)
                || !ids.insert(&a.artifact_id)
                || !names.insert(&a.filename)
                || a.filename.is_empty()
                || a.filename.len() > 255
                || a.filename.contains("..")
                || a.filename.starts_with('.')
                || a.filename.ends_with('.')
                || !a
                    .filename
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                || a.size_bytes > MAX_ARTIFACT_BYTES
            {
                return Err(Error::Schema);
            }
            let stem = a
                .filename
                .split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(Error::Schema);
            }
            if a.sha256.is_empty() {
                return Err(Error::MissingDigest);
            }
            if a.sha256.len() != 64
                || !a
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Schema);
            }
        }
        Ok(m)
    }
}
pub fn signing_input(bytes: &[u8]) -> Vec<u8> {
    [DOMAIN, bytes].concat()
}
