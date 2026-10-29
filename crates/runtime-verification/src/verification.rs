use crate::*;
use endfield_unix_runtime::*;
use std::fs::File;

/// Explicit artifact binding supplied independently of the manifest. The file name
/// is a logical selected name; the CLI also checks its selected path's basename.
pub struct ArtifactInput<'a> {
    pub artifact_id: &'a str,
    pub filename: &'a str,
    pub build: &'a str,
    pub file: File,
    pub metadata: &'a VerifiedManifest,
}
#[derive(Debug)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub manifest_id: String,
    pub manifest_version: u64,
    pub channel: String,
    pub key_id: String,
    pub source: SourceMetadata,
    pub assessment: IntegrityAssessment,
    pub provenance: Provenance,
    pub expiration: ExpirationStatus,
    pub rollback: RollbackStatus,
    pub size: u64,
}
pub fn verify_artifact(input: ArtifactInput<'_>) -> Result<VerificationReport, Error> {
    let v = input.metadata;
    let m = v.manifest();
    if input.build != m.runtime_build {
        return Err(Error::Identity);
    }
    let a = m
        .artifacts
        .iter()
        .find(|a| a.artifact_id == input.artifact_id)
        .ok_or(Error::Identity)?;
    if input.filename != a.filename {
        return Err(Error::Identity);
    }
    let actual = hash_artifact(input.file)?;
    if actual.size != a.size_bytes {
        return Err(Error::SizeMismatch);
    }
    let trusted =
        v.provenance() == Provenance::IndependentlyAuthenticatedPublisher && v.time.is_some();
    let authority = if trusted {
        DigestAuthority::AuthenticatedManifest(ManifestId::new(&m.manifest_id)?)
    } else {
        DigestAuthority::UserSupplied
    };
    let assessment = IntegrityRecord::new(
        ArtifactId::new(&a.artifact_id)?,
        Some(ExpectedDigest::new(
            Digest::new(DigestAlgorithm::Sha256, &a.sha256)?,
            authority,
        )),
        Some(Digest::new(DigestAlgorithm::Sha256, actual.sha256)?),
    )
    .assess();
    if assessment.outcome() != IntegrityOutcome::DigestMatches {
        return Err(Error::DigestMismatch);
    }
    let evidence = EvidenceId::new(format!("sha256-{}", v.content_sha256))?;
    let at = v.time.and_then(|t| u64::try_from(t).ok());
    let verification = if trusted {
        let proof = AuthenticityProof::new(
            VerificationMethod::DetachedSignature,
            evidence.clone(),
            at.map(VerificationTimestamp::new),
        )?;
        SourceVerification::new(
            SourceVerificationState::ArtifactIntegrityVerified,
            AuthenticityState::Established(proof),
            MetadataVerification::Verified {
                evidence_id: evidence.clone(),
                verified_at: at.map(VerificationTimestamp::new),
            },
            Some(ArtifactIntegrityVerification::new(
                evidence,
                assessment.clone(),
                at,
            )?),
            None,
            None,
        )?
    } else {
        SourceVerification::new(
            SourceVerificationState::CandidateIdentified,
            AuthenticityState::Claimed(VerificationMethod::UserAssertion),
            MetadataVerification::NotVerified,
            None,
            None,
            None,
        )?
    };
    let source = SourceMetadata::new(
        SourceIdentity::new(
            ProviderId::new(&m.provider_id)?,
            if trusted {
                SourceCategory::VerifiedProjectRelease
            } else {
                SourceCategory::UserSuppliedArtifact
            },
            Some(ProjectId::new(&m.project_id)?),
            Some(Identifier::new(&m.runtime_build)?),
            None,
        )?,
        verification,
    )?;
    Ok(VerificationReport {
        schema_version: m.schema_version,
        manifest_id: m.manifest_id.clone(),
        manifest_version: m.manifest_version,
        channel: m.channel.clone(),
        key_id: m.key_id.clone(),
        source,
        assessment,
        provenance: v.provenance(),
        expiration: v.expiration(),
        rollback: v.rollback(),
        size: actual.size,
    })
}
/// P021 request adapter: it can only expose already verified evidence, never hash
/// bytes from a request that carries no file. No discovery provider is upgraded.
impl VerificationReport {
    pub fn assess_request(
        &self,
        request: &IntegrityVerificationRequest,
    ) -> Result<IntegrityAssessment, Error> {
        if request.artifact_id() != self.assessment.artifact_id()
            || request.expected() != self.assessment.expected()
        {
            return Err(Error::Identity);
        }
        Ok(self.assessment.clone())
    }
    pub fn verify_source_request(
        &self,
        request: &SourceVerificationRequest,
    ) -> Result<SourceVerification, Error> {
        if request.source().identity() != self.source.identity() {
            return Err(Error::Identity);
        }
        Ok(self.source.verification().clone())
    }
    pub fn capabilities() -> [ProviderCapabilityKind; 2] {
        [
            ProviderCapabilityKind::VerifySource,
            ProviderCapabilityKind::VerifyIntegrity,
        ]
    }
}

/// Read-only P021 adapter bound to one explicitly verified artifact. Construction
/// performs verification; bare P021 requests never cause implicit filesystem reads.
pub struct VerifiedArtifactProvider {
    declaration: ProviderDeclaration,
    report: VerificationReport,
}
impl VerifiedArtifactProvider {
    pub fn verify(input: ArtifactInput<'_>) -> Result<Self, Error> {
        let family = match input.metadata.manifest().runtime_family.as_str() {
            "wine" => RuntimeFamily::Wine,
            _ => RuntimeFamily::Proton,
        };
        let report = verify_artifact(input)?;
        let declaration = ProviderDeclaration::new(
            report.source.identity().provider_id().clone(),
            ProviderAvailability::Available,
            vec![family],
            VerificationReport::capabilities()
                .into_iter()
                .map(|kind| ProviderCapability::new(kind, CapabilityStatus::VerifiedUsable))
                .collect(),
        )?;
        Ok(Self {
            declaration,
            report,
        })
    }
    pub fn report(&self) -> &VerificationReport {
        &self.report
    }
    fn incompatible(&self, operation: ProviderOperation) -> ProviderError {
        ProviderError::new(
            self.declaration.provider_id().identifier().clone(),
            operation,
            ProviderErrorKind::IncompatibleRequest,
        )
    }
}
impl RuntimeProvider for VerifiedArtifactProvider {
    fn declaration(&self) -> &ProviderDeclaration {
        &self.declaration
    }
    fn verify_source_impl(
        &self,
        request: &SourceVerificationRequest,
    ) -> Result<SourceVerification, ProviderError> {
        self.report
            .verify_source_request(request)
            .map_err(|_| self.incompatible(ProviderOperation::VerifySource))
    }
    fn verify_integrity_impl(
        &self,
        request: &IntegrityVerificationRequest,
    ) -> Result<IntegrityAssessment, ProviderError> {
        self.report
            .assess_request(request)
            .map_err(|_| self.incompatible(ProviderOperation::VerifyIntegrity))
    }
}
