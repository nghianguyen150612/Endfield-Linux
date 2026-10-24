use endfield_unix_domain::{DistributionIdentity, ValidationError};

use crate::os_release::OsReleaseData;

/// Bounded distribution metadata retained by the Linux-specific report.
/// P007's portable identity receives ID, version, and display name only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDistributionMetadata {
    pub id: String,
    pub name: Option<String>,
    pub pretty_name: Option<String>,
    pub version_id: Option<String>,
    pub version: Option<String>,
    pub id_like: Vec<String>,
    pub variant_id: Option<String>,
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DistributionNormalizeError {
    MissingId,
    InvalidDomainValue,
}

impl LinuxDistributionMetadata {
    pub(crate) fn from_os_release(
        data: OsReleaseData,
        source: &'static str,
    ) -> Result<Self, DistributionNormalizeError> {
        let id = data.id.ok_or(DistributionNormalizeError::MissingId)?;
        let metadata = Self {
            id,
            name: data.name,
            pretty_name: data.pretty_name,
            version_id: data.version_id,
            version: data.version,
            id_like: data.id_like,
            variant_id: data.variant_id,
            source,
        };
        metadata
            .distribution_identity()
            .map_err(|_| DistributionNormalizeError::InvalidDomainValue)?;
        Ok(metadata)
    }

    pub fn distribution_identity(&self) -> Result<DistributionIdentity, ValidationError> {
        let version = self.version_id.clone().or_else(|| self.version.clone());
        let display_name = self.pretty_name.clone().or_else(|| self.name.clone());
        DistributionIdentity::new(self.id.clone(), version, display_name)
    }
}
