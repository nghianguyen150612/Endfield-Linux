use std::collections::BTreeSet;

use crate::errors::TelemetryError;
use crate::observation::{ObservationIdentityTime, TelemetryObservation};
use crate::provenance::{ProviderId, SamplingContextId};
use crate::scope::{ObservationTarget, SnapshotContext};
use crate::timestamp::ClockDomainId;

pub const CURRENT_TELEMETRY_SCHEMA_VERSION: u16 = 1;
pub const MAX_OBSERVATIONS_PER_BATCH: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryBatch {
    schema_version: u16,
    sampling_context: SamplingContextId,
    clock_domain: Option<ClockDomainId>,
    snapshot: Option<SnapshotContext>,
    observations: Vec<TelemetryObservation>,
}

impl TelemetryBatch {
    pub fn new(
        sampling_context: SamplingContextId,
        clock_domain: Option<ClockDomainId>,
        snapshot: Option<SnapshotContext>,
        observations: Vec<TelemetryObservation>,
    ) -> Result<Self, TelemetryError> {
        Self::with_schema_version(
            CURRENT_TELEMETRY_SCHEMA_VERSION,
            sampling_context,
            clock_domain,
            snapshot,
            observations,
        )
    }

    pub fn with_schema_version(
        schema_version: u16,
        sampling_context: SamplingContextId,
        clock_domain: Option<ClockDomainId>,
        snapshot: Option<SnapshotContext>,
        mut observations: Vec<TelemetryObservation>,
    ) -> Result<Self, TelemetryError> {
        if schema_version != CURRENT_TELEMETRY_SCHEMA_VERSION {
            return Err(TelemetryError::UnsupportedSchemaVersion {
                found: schema_version,
                supported: CURRENT_TELEMETRY_SCHEMA_VERSION,
            });
        }
        let batch = Self {
            schema_version,
            sampling_context,
            clock_domain,
            snapshot,
            observations: Vec::new(),
        };
        if observations.len() > MAX_OBSERVATIONS_PER_BATCH {
            return Err(TelemetryError::BatchTooLarge {
                count: observations.len(),
                maximum: MAX_OBSERVATIONS_PER_BATCH,
            });
        }
        for observation in &observations {
            observation.validate(batch.snapshot.as_ref())?;
            if target_requires_snapshot(observation.target()) && batch.snapshot.is_none() {
                return Err(TelemetryError::UnresolvedDeviceReference);
            }
        }
        validate_clock_domains(clock_domain, &observations)?;
        observations.sort_by_key(sort_key);
        validate_no_duplicates(&observations)?;
        Ok(Self {
            observations,
            ..batch
        })
    }

    pub fn validate(&self) -> Result<(), TelemetryError> {
        if self.schema_version != CURRENT_TELEMETRY_SCHEMA_VERSION {
            return Err(TelemetryError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CURRENT_TELEMETRY_SCHEMA_VERSION,
            });
        }
        if self.observations.len() > MAX_OBSERVATIONS_PER_BATCH {
            return Err(TelemetryError::BatchTooLarge {
                count: self.observations.len(),
                maximum: MAX_OBSERVATIONS_PER_BATCH,
            });
        }
        for observation in &self.observations {
            observation.validate(self.snapshot.as_ref())?;
            if target_requires_snapshot(observation.target()) && self.snapshot.is_none() {
                return Err(TelemetryError::UnresolvedDeviceReference);
            }
        }
        validate_clock_domains(self.clock_domain, &self.observations)?;
        validate_no_duplicates(&self.observations)?;
        if self
            .observations
            .windows(2)
            .any(|window| sort_key(&window[0]) > sort_key(&window[1]))
        {
            return Err(TelemetryError::InvalidValue {
                field: "telemetry batch order",
                reason: "observations are not in deterministic normalized order",
            });
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn sampling_context(&self) -> SamplingContextId {
        self.sampling_context
    }

    pub const fn clock_domain(&self) -> Option<ClockDomainId> {
        self.clock_domain
    }

    pub const fn snapshot_context(&self) -> Option<&SnapshotContext> {
        self.snapshot.as_ref()
    }

    pub fn observations(&self) -> &[TelemetryObservation] {
        &self.observations
    }
}

fn target_requires_snapshot(target: ObservationTarget) -> bool {
    matches!(
        target,
        ObservationTarget::Cpu(_)
            | ObservationTarget::LogicalCpu { .. }
            | ObservationTarget::Gpu(_)
            | ObservationTarget::GpuEngine { .. }
    )
}

fn validate_clock_domains(
    expected: Option<ClockDomainId>,
    observations: &[TelemetryObservation],
) -> Result<(), TelemetryError> {
    let mut found = BTreeSet::new();
    for observation in observations {
        if let Some(domain) = observation.temporal().clock_domain() {
            found.insert(domain);
        }
    }
    if found.len() > 1 || found.iter().any(|domain| Some(*domain) != expected) {
        return Err(TelemetryError::ClockDomainMismatch);
    }
    Ok(())
}

fn sort_key(
    observation: &TelemetryObservation,
) -> (u16, ObservationTarget, ProviderId, ObservationIdentityTime) {
    (
        observation.metric().stable_id(),
        observation.target(),
        observation.provenance().provider().clone(),
        observation.observation_key(),
    )
}

fn validate_no_duplicates(observations: &[TelemetryObservation]) -> Result<(), TelemetryError> {
    let mut identities = BTreeSet::new();
    for observation in observations {
        let identity = (
            observation.metric(),
            observation.target(),
            observation.provenance().provider().clone(),
            observation.observation_key(),
        );
        if !identities.insert(identity) {
            return Err(TelemetryError::DuplicateObservation);
        }
    }
    Ok(())
}
