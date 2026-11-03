//! Official pages establish listing provenance, never installed payload authenticity.
use serde::Serialize;
use thiserror::Error;

#[cfg(target_os = "linux")]
mod steam;
#[cfg(target_os = "linux")]
pub use steam::{inspect_steam, revalidate_steam};

pub const SCHEMA_VERSION: u32 = 1;
pub const CATALOG_REVISION: &str = "2026-11-02.1";
pub const STEAM_APP_ID: &str = "4732690";
pub const OBSERVED_ON: &str = "2026-11-02";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameDistributionChannel {
    Steam,
    Epic,
    Gryphlink,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceVerificationStatus {
    VerifiedOfficialListing,
    VerifiedPublisherAnnouncement,
    ObservedLocalMetadata,
    UserDeclared,
    Unverifiable,
    Unsupported,
    Changed,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCapability {
    CatalogOnly,
    OfficialHandoff,
    LocalInstallationObservation,
    VerifiedArtifactAcquisition,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceEvidence {
    pub id: &'static str,
    pub title: &'static str,
    pub provider: &'static str,
    pub url: &'static str,
    /// Source-displayed publication date, not observation or game release date.
    pub publication_date: Option<&'static str>,
    pub observed_on: &'static str,
    pub status: SourceVerificationStatus,
    pub claim: &'static str,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OfficialGameSource {
    pub id: &'static str,
    pub channel: GameDistributionChannel,
    pub display_name: &'static str,
    pub official_page: &'static str,
    pub publisher: &'static str,
    pub developer: &'static str,
    pub platform_reported: &'static str,
    pub region_scope: &'static str,
    pub restrictions: &'static str,
    pub capabilities: Vec<SourceCapability>,
    pub unsupported_operations: &'static [&'static str],
    pub trust: SourceVerificationStatus,
    pub evidence: Vec<SourceEvidence>,
    pub guidance: &'static str,
}
#[derive(Debug, Serialize)]
pub struct SourceCatalog {
    pub schema_version: u32,
    pub revision: &'static str,
    pub verified_on: &'static str,
    pub live_availability: SourceVerificationStatus,
    pub sources: Vec<OfficialGameSource>,
}
const UNSUPPORTED: &[&str] = &[
    "direct_download",
    "artifact_acquisition",
    "launcher_execution",
    "updates",
    "channel_conversion",
    "authentication",
];
fn evidence(
    id: &'static str,
    title: &'static str,
    provider: &'static str,
    url: &'static str,
    date: Option<&'static str>,
    status: SourceVerificationStatus,
    claim: &'static str,
) -> SourceEvidence {
    SourceEvidence {
        id,
        title,
        provider,
        url,
        publication_date: date,
        observed_on: OBSERVED_ON,
        status,
        claim,
    }
}
/// Small built-in reviewed catalog. No networking or filesystem access.
pub fn catalog() -> SourceCatalog {
    use SourceCapability::*;
    use SourceVerificationStatus::*;
    SourceCatalog {
        schema_version: SCHEMA_VERSION,
        revision: CATALOG_REVISION,
        verified_on: OBSERVED_ON,
        live_availability: Unknown,
        sources: vec![
            OfficialGameSource {
                id: "epic",
                channel: GameDistributionChannel::Epic,
                display_name: "Epic Games Store",
                official_page: "https://store.epicgames.com/p/arknightsendfield-f0f167",
                publisher: "GRYPHLINE",
                developer: "Hypergryph",
                platform_reported: "Windows 10/11 64-bit",
                region_scope: "Global listing; availability and account restrictions are controlled by Epic",
                restrictions: "Epic ID required; no internal catalog ID or local manifest protocol established",
                capabilities: vec![CatalogOnly, OfficialHandoff],
                unsupported_operations: UNSUPPORTED,
                trust: VerifiedOfficialListing,
                evidence: vec![evidence(
                    "epic-listing",
                    "Arknights: Endfield | Epic Games Store",
                    "Epic Games Store",
                    "https://store.epicgames.com/p/arknightsendfield-f0f167",
                    None,
                    VerifiedOfficialListing,
                    "Official store listing, developer/publisher attribution and Windows requirements; publication date not displayed",
                )],
                guidance: "Visit the official Epic listing and follow Epic's supported installation instructions on its supported platform. Epic owns installation and updates. Local Epic metadata inspection is unsupported.",
            },
            OfficialGameSource {
                id: "gryphlink",
                channel: GameDistributionChannel::Gryphlink,
                display_name: "GRYPHLINK (publisher channel)",
                official_page: "https://endfield.gryphline.com/",
                publisher: "GRYPHLINE",
                developer: "Hypergryph",
                platform_reported: "PC (publisher update notice); no Linux support established",
                region_scope: "Global GRYPHLINE; no Chinese-build equivalence established",
                restrictions: "Standalone publisher channel; cannot manage the Steam edition",
                capabilities: vec![CatalogOnly, OfficialHandoff],
                unsupported_operations: UNSUPPORTED,
                trust: VerifiedPublisherAnnouncement,
                evidence: vec![
                    evidence(
                        "gryphlink-update",
                        "[Sanctuary of Ink] Version Pre-Download & Update Notice",
                        "GRYPHLINE",
                        "https://endfield.gryphline.com/en-us/news/0744",
                        Some("2026-10-13 12:00 (timezone not displayed)"),
                        VerifiedPublisherAnnouncement,
                        "Publisher directs PC pre-download and updates through GRYPHLINK; not a public download API",
                    ),
                    evidence(
                        "publisher-home",
                        "Arknights: Endfield",
                        "GRYPHLINE",
                        "https://endfield.gryphline.com/",
                        None,
                        VerifiedOfficialListing,
                        "Publisher game landing page; observed redirect to /en-us; no artifact protocol established",
                    ),
                    evidence(
                        "publisher-attribution",
                        "Arknights: Endfield on Steam",
                        "Steam",
                        "https://store.steampowered.com/app/4732690/",
                        None,
                        VerifiedOfficialListing,
                        "Hypergryph developer and GRYPHLINE publisher attribution only",
                    ),
                ],
                guidance: "Visit the publisher game page for supported installation guidance. The publisher's PC update notice directs users to close the game, restart GRYPHLINK and select Update. Endfield-Unix does not install or run GRYPHLINK. Use Steam for the Steam edition.",
            },
            OfficialGameSource {
                id: "steam",
                channel: GameDistributionChannel::Steam,
                display_name: "Steam",
                official_page: "https://store.steampowered.com/app/4732690/",
                publisher: "GRYPHLINE",
                developer: "Hypergryph",
                platform_reported: "Windows 10/11 64-bit",
                region_scope: "Publisher announcement: Asia and Americas / Europe servers; no Chinese-build equivalence established",
                restrictions: "Steam owns installation and updates. Store lists kernel-level ACE anti-cheat; The task reports Steam Deck Unsupported; the badge was not exposed by fetched text and is not independently revalidated. No Linux qualification.",
                capabilities: vec![CatalogOnly, OfficialHandoff, LocalInstallationObservation],
                unsupported_operations: UNSUPPORTED,
                trust: VerifiedOfficialListing,
                evidence: vec![
                    evidence(
                        "steam-listing",
                        "Arknights: Endfield on Steam",
                        "Steam",
                        "https://store.steampowered.com/app/4732690/",
                        None,
                        VerifiedOfficialListing,
                        "App 4732690, Hypergryph developer, GRYPHLINE publisher, Windows requirements and kernel-level ACE anti-cheat; release date is not publication date",
                    ),
                    evidence(
                        "steam-announcement",
                        "Arknights: Endfield Is Now Officially Available on Steam!",
                        "GRYPHLINE",
                        "https://endfield.gryphline.com/vi-vn/news/3837",
                        Some("2026-10-14 20:00 (timezone not displayed)"),
                        VerifiedPublisherAnnouncement,
                        "Publisher links Steam app 4732690, names supported servers and Windows 64-bit requirements",
                    ),
                ],
                guidance: "Visit the official Steam listing and use Steam's supported installation workflow on its supported platform. Steam owns updates; do not use GRYPHLINK to update this edition. Inspect one explicitly selected library root or appmanifest_4732690.acf; then explicitly import the observed directory with game import plan --declare-endfield. No Linux gameplay support is established.",
            },
        ],
    }
}
pub fn source(id: &str) -> Result<OfficialGameSource, SourceError> {
    catalog()
        .sources
        .into_iter()
        .find(|source| source.id == id)
        .ok_or(SourceError::UnknownSource)
}
/// Only built-in identity/capabilities are revalidated; historical evidence is not a live availability check.
pub fn validate_catalog(value: &SourceCatalog) -> Result<(), SourceError> {
    let expected = catalog();
    if value.schema_version != SCHEMA_VERSION
        || value.revision != CATALOG_REVISION
        || value.sources.len() != 3
        || value.verified_on != OBSERVED_ON
        || value.live_availability != SourceVerificationStatus::Unknown
    {
        return Err(SourceError::InvalidCatalog);
    }
    for (actual, expected) in value.sources.iter().zip(expected.sources) {
        if *actual != expected {
            return Err(SourceError::InvalidCatalog);
        }
    }
    Ok(())
}
#[derive(Debug, Error)]
pub enum SourceError {
    #[error("unknown official source ID")]
    UnknownSource,
    #[error("catalog identity or capability policy mismatch")]
    InvalidCatalog,
    #[error("local inspection is unsupported for this source or platform")]
    Unsupported,
    #[error("unsafe or invalid selected path")]
    InvalidPath,
    #[error("selected source metadata or installation is missing")]
    Missing,
    #[error("malformed or unsupported bounded Steam metadata")]
    InvalidMetadata,
    #[error("Steam app identity does not match 4732690")]
    WrongApp,
    #[error("source metadata or directory identity changed")]
    Changed,
    #[error("source filesystem access failed")]
    Io,
}
impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownSource => "unknown_source",
            Self::InvalidCatalog => "invalid_catalog",
            Self::Unsupported => "unsupported_source_operation",
            Self::InvalidPath => "unsafe_source_path",
            Self::Missing => "source_missing",
            Self::InvalidMetadata => "invalid_source_metadata",
            Self::WrongApp => "wrong_steam_app",
            Self::Changed => "source_changed",
            Self::Io => "source_io",
        }
    }
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::UnknownSource | Self::InvalidPath | Self::WrongApp => 2,
            Self::Missing => 3,
            Self::Changed => 4,
            Self::InvalidCatalog => 70,
            Self::Io => 74,
            Self::Unsupported | Self::InvalidMetadata => 78,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceObjectIdentity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstalledSourceEvidence {
    pub schema_version: u32,
    pub source_id: &'static str,
    pub app_id: &'static str,
    pub status: SourceVerificationStatus,
    pub manifest_path: String,
    pub installation_path: String,
    pub installation_present: bool,
    pub installation_identity: Option<SourceObjectIdentity>,
    pub reported_build_id: Option<String>,
    pub reported_state_flags: Option<String>,
    pub observation_token: String,
    pub publisher_authenticity: SourceVerificationStatus,
    pub artifact_integrity: SourceVerificationStatus,
    pub game_build: SourceVerificationStatus,
    pub game_compatibility: &'static str,
    pub execution_readiness: SourceVerificationStatus,
    pub limitations: &'static str,
}
