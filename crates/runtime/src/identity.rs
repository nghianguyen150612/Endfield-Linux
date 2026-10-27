use std::collections::BTreeSet;
use std::fmt;

use crate::errors::ContractError;
use crate::integrity::Digest;
use crate::source::SourceMetadata;
use crate::validation::{MAX_IDENTIFIER_BYTES, validate_identifier, validate_text};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier(String);

impl Identifier {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        validate_identifier(&value, "opaque")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! identifier_type {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Identifier);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                validate_identifier(&value, $field)?;
                Ok(Self(Identifier(value)))
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            pub fn identifier(&self) -> &Identifier {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

identifier_type!(ProviderId, "provider");
identifier_type!(RuntimeId, "runtime");
identifier_type!(EvidenceId, "evidence");
identifier_type!(ProjectId, "project");
identifier_type!(ProfileId, "profile");
identifier_type!(ArtifactId, "artifact");
identifier_type!(ManifestId, "manifest");
identifier_type!(GameBuildId, "game build");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeFamily {
    Wine,
    Proton,
    Unknown,
}

impl RuntimeFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wine => "wine",
            Self::Proton => "proton",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeLineage {
    UpstreamWine,
    WineStaging,
    ValveProton,
    GeProton,
    DwProton,
    OtherVerifiedDerivative(Identifier),
    Unknown,
}

impl RuntimeLineage {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UpstreamWine => "upstream-wine",
            Self::WineStaging => "wine-staging",
            Self::ValveProton => "valve-proton",
            Self::GeProton => "ge-proton",
            Self::DwProton => "dw-proton",
            Self::OtherVerifiedDerivative(_) => "other-verified-derivative",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn is_valid_for(&self, family: RuntimeFamily) -> bool {
        matches!(
            (family, self),
            (
                RuntimeFamily::Wine,
                Self::UpstreamWine | Self::WineStaging | Self::Unknown
            ) | (
                RuntimeFamily::Proton,
                Self::ValveProton | Self::GeProton | Self::DwProton | Self::Unknown,
            ) | (
                RuntimeFamily::Wine | RuntimeFamily::Proton,
                Self::OtherVerifiedDerivative(_)
            ) | (RuntimeFamily::Unknown, Self::Unknown)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeVersion(String);

impl RuntimeVersion {
    pub const MAX_BYTES: usize = 160;

    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        validate_text(&value, "runtime version", Self::MAX_BYTES)?;
        Ok(Self(value))
    }

    /// Returns the provider's raw version label without SemVer conversion.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuntimeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessArchitecture {
    X86,
    X86_64,
    Aarch64,
    Arm,
    Riscv64,
    Unknown,
}

impl ProcessArchitecture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Arm => "arm",
            Self::Riscv64 => "riscv64",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimePlatform {
    Linux,
    Windows,
    MacOs,
    FreeBsd,
    Unknown,
}

impl RuntimePlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::FreeBsd => "freebsd",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeRequirements {
    platforms: BTreeSet<RuntimePlatform>,
    architectures: BTreeSet<ProcessArchitecture>,
}

impl RuntimeRequirements {
    pub fn new(
        platforms: impl IntoIterator<Item = RuntimePlatform>,
        architectures: impl IntoIterator<Item = ProcessArchitecture>,
    ) -> Result<Self, ContractError> {
        let platforms = collect_bounded(platforms)?;
        let architectures = collect_bounded(architectures)?;
        Ok(Self {
            platforms,
            architectures,
        })
    }

    pub fn platforms(&self) -> &BTreeSet<RuntimePlatform> {
        &self.platforms
    }

    pub fn architectures(&self) -> &BTreeSet<ProcessArchitecture> {
        &self.architectures
    }

    /// An absent answer means the provider supplied no constraint or the request is unknown.
    pub fn supports_platform(&self, platform: RuntimePlatform) -> Option<bool> {
        if self.platforms.is_empty() || platform == RuntimePlatform::Unknown {
            None
        } else {
            Some(self.platforms.contains(&platform))
        }
    }

    /// An absent answer means the provider supplied no constraint or the request is unknown.
    pub fn supports_architecture(&self, architecture: ProcessArchitecture) -> Option<bool> {
        if self.architectures.is_empty() || architecture == ProcessArchitecture::Unknown {
            None
        } else {
            Some(self.architectures.contains(&architecture))
        }
    }
}

fn collect_bounded<T: Ord>(
    values: impl IntoIterator<Item = T>,
) -> Result<BTreeSet<T>, ContractError> {
    let mut collected = BTreeSet::new();
    for (index, value) in values.into_iter().enumerate() {
        if index >= 16 {
            return Err(ContractError::MetadataTooLarge);
        }
        collected.insert(value);
    }
    Ok(collected)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeOwnership {
    EndfieldManaged,
    ExternalUserManaged,
    DistributionManaged,
    OtherLauncherManaged(ProviderId),
    Unknown,
}

impl RuntimeOwnership {
    pub const fn is_endfield_managed(&self) -> bool {
        matches!(self, Self::EndfieldManaged)
    }

    pub const fn is_explicitly_external(&self) -> bool {
        matches!(
            self,
            Self::ExternalUserManaged | Self::DistributionManaged | Self::OtherLauncherManaged(_)
        )
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::EndfieldManaged => "endfield-managed",
            Self::ExternalUserManaged => "external-user-managed",
            Self::DistributionManaged => "distribution-managed",
            Self::OtherLauncherManaged(_) => "other-launcher-managed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBuildIdentity {
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    provider_id: ProviderId,
    version: RuntimeVersion,
    build_channel: Option<String>,
    requirements: RuntimeRequirements,
    integrity_identity: Option<Digest>,
}

impl RuntimeBuildIdentity {
    pub const fn family(&self) -> RuntimeFamily {
        self.family
    }

    pub const fn lineage(&self) -> &RuntimeLineage {
        &self.lineage
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn version(&self) -> &RuntimeVersion {
        &self.version
    }

    pub fn build_channel(&self) -> Option<&str> {
        self.build_channel.as_deref()
    }

    pub const fn requirements(&self) -> &RuntimeRequirements {
        &self.requirements
    }

    pub const fn integrity_identity(&self) -> Option<&Digest> {
        self.integrity_identity.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeIdentity {
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    provider_id: ProviderId,
    version: RuntimeVersion,
    build_channel: Option<String>,
    requirements: RuntimeRequirements,
    source: Option<SourceMetadata>,
    integrity_identity: Option<Digest>,
    ownership: RuntimeOwnership,
}

impl RuntimeIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        family: RuntimeFamily,
        lineage: RuntimeLineage,
        provider_id: ProviderId,
        version: RuntimeVersion,
        build_channel: Option<String>,
        requirements: RuntimeRequirements,
        source: Option<SourceMetadata>,
        integrity_identity: Option<Digest>,
        ownership: RuntimeOwnership,
    ) -> Result<Self, ContractError> {
        if !lineage.is_valid_for(family) {
            return Err(ContractError::InvalidValue {
                field: "runtime lineage",
                reason: "lineage does not match runtime family",
            });
        }
        if let Some(channel) = &build_channel {
            validate_text(channel, "runtime build channel", 128)?;
        }
        let metadata_bytes = provider_id.as_str().len()
            + version.as_str().len()
            + build_channel.as_ref().map_or(0, String::len)
            + requirements.platforms.len() * 16
            + requirements.architectures.len() * 16;
        if metadata_bytes > MAX_IDENTIFIER_BYTES * 4 {
            return Err(ContractError::MetadataTooLarge);
        }
        if let Some(source_metadata) = &source {
            if source_metadata
                .verification()
                .state()
                .has_verified_artifact()
            {
                let verified_actual = source_metadata
                    .verification()
                    .artifact_integrity()
                    .and_then(|verification| verification.assessment().actual());
                if integrity_identity.as_ref() != verified_actual {
                    return Err(ContractError::InvalidVerificationTransition);
                }
            }
        }
        Ok(Self {
            family,
            lineage,
            provider_id,
            version,
            build_channel,
            requirements,
            source,
            integrity_identity,
            ownership,
        })
    }

    pub const fn family(&self) -> RuntimeFamily {
        self.family
    }

    pub const fn lineage(&self) -> &RuntimeLineage {
        &self.lineage
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn version(&self) -> &RuntimeVersion {
        &self.version
    }

    pub fn build_channel(&self) -> Option<&str> {
        self.build_channel.as_deref()
    }

    pub const fn requirements(&self) -> &RuntimeRequirements {
        &self.requirements
    }

    pub const fn source(&self) -> Option<&SourceMetadata> {
        self.source.as_ref()
    }

    pub const fn integrity_identity(&self) -> Option<&Digest> {
        self.integrity_identity.as_ref()
    }

    pub const fn ownership(&self) -> &RuntimeOwnership {
        &self.ownership
    }

    pub fn source_integrity_matches_identity(&self) -> bool {
        let Some(source) = &self.source else {
            return false;
        };
        let Some(verification) = source.verification().artifact_integrity() else {
            return false;
        };
        self.integrity_identity.as_ref() == verification.assessment().actual()
            && verification.is_from_authenticated_metadata()
    }

    pub fn build_identity(&self) -> RuntimeBuildIdentity {
        RuntimeBuildIdentity {
            family: self.family,
            lineage: self.lineage.clone(),
            provider_id: self.provider_id.clone(),
            version: self.version.clone(),
            build_channel: self.build_channel.clone(),
            requirements: self.requirements.clone(),
            integrity_identity: self.integrity_identity.clone(),
        }
    }

    /// A version label alone is deliberately insufficient to identify identical bytes.
    pub fn same_verified_build(&self, other: &Self) -> bool {
        self.integrity_identity.is_some()
            && self.integrity_identity == other.integrity_identity
            && self.build_identity() == other.build_identity()
    }
}
