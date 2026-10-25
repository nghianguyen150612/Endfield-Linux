use std::collections::BTreeMap;

use endfield_unix_domain::{InstanceId, SystemSnapshot};

use crate::errors::TelemetryError;
use crate::metric::{MetricId, MetricScope};
use crate::provenance::{SamplingContextId, SnapshotRefId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceReference {
    snapshot: SnapshotRefId,
    instance: InstanceId,
}

impl DeviceReference {
    pub const fn new(snapshot: SnapshotRefId, instance: InstanceId) -> Self {
        Self { snapshot, instance }
    }

    pub const fn snapshot(self) -> SnapshotRefId {
        self.snapshot
    }

    pub const fn instance(self) -> InstanceId {
        self.instance
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservationTarget {
    Platform,
    Cpu(DeviceReference),
    LogicalCpu {
        cpu: DeviceReference,
        logical_index: u32,
    },
    Process(SamplingContextId),
    Cgroup(SamplingContextId),
    Gpu(DeviceReference),
    GpuEngine {
        gpu: DeviceReference,
        engine: GpuEngineKind,
    },
    GameSession(SamplingContextId),
    Unassociated(MetricScope),
}

impl ObservationTarget {
    pub const fn scope(self) -> MetricScope {
        match self {
            Self::Platform => MetricScope::Platform,
            Self::Cpu(_) => MetricScope::Cpu,
            Self::LogicalCpu { .. } => MetricScope::LogicalCpu,
            Self::Process(_) => MetricScope::Process,
            Self::Cgroup(_) => MetricScope::Cgroup,
            Self::Gpu(_) => MetricScope::Gpu,
            Self::GpuEngine { .. } => MetricScope::GpuEngine,
            Self::GameSession(_) => MetricScope::GameSession,
            Self::Unassociated(scope) => scope,
        }
    }
}

/// Fixed labels avoid retaining arbitrary GPU engine strings or driver data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GpuEngineKind {
    Graphics,
    Compute,
    Copy,
    VideoDecode,
    VideoEncode,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotDeviceKind {
    Cpu,
    Gpu,
    Display,
}

/// A validated, snapshot-local device catalog for resolving telemetry targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotContext {
    reference: SnapshotRefId,
    devices: BTreeMap<InstanceId, SnapshotDeviceKind>,
    logical_cpu_count: Option<u32>,
}

impl SnapshotContext {
    pub fn from_snapshot(
        reference: SnapshotRefId,
        snapshot: &SystemSnapshot,
    ) -> Result<Self, TelemetryError> {
        snapshot
            .validate()
            .map_err(|_| TelemetryError::InvalidSnapshotContext)?;
        let mut devices = BTreeMap::new();
        let mut logical_cpu_count = None;
        if let Some(cpu) = &snapshot.cpu {
            devices.insert(cpu.instance, SnapshotDeviceKind::Cpu);
            logical_cpu_count = cpu
                .topology
                .value
                .as_ref()
                .map(|topology| topology.logical_processor_count);
        }
        for gpu in &snapshot.gpus {
            devices.insert(gpu.instance, SnapshotDeviceKind::Gpu);
        }
        for display in &snapshot.displays {
            devices.insert(display.instance, SnapshotDeviceKind::Display);
        }
        Ok(Self {
            reference,
            devices,
            logical_cpu_count,
        })
    }

    pub const fn reference(&self) -> SnapshotRefId {
        self.reference
    }

    pub(crate) fn validate_target(
        &self,
        metric: MetricId,
        target: ObservationTarget,
    ) -> Result<(), TelemetryError> {
        let (device, expected_kind, logical_index) = match target {
            ObservationTarget::Cpu(device) => (Some(device), Some(SnapshotDeviceKind::Cpu), None),
            ObservationTarget::LogicalCpu { cpu, logical_index } => (
                Some(cpu),
                Some(SnapshotDeviceKind::Cpu),
                Some(logical_index),
            ),
            ObservationTarget::Gpu(device) => (Some(device), Some(SnapshotDeviceKind::Gpu), None),
            ObservationTarget::GpuEngine { gpu, .. } => {
                (Some(gpu), Some(SnapshotDeviceKind::Gpu), None)
            }
            _ => (None, None, None),
        };
        if let Some(device) = device {
            if device.snapshot() != self.reference {
                return Err(TelemetryError::SnapshotReferenceMismatch);
            }
            if self.devices.get(&device.instance()).copied() != expected_kind {
                return Err(TelemetryError::UnresolvedDeviceReference);
            }
            if let Some(index) = logical_index {
                if self.logical_cpu_count.is_none_or(|count| index >= count) {
                    return Err(TelemetryError::UnresolvedDeviceReference);
                }
            }
        }
        if target.scope() != metric.definition().scope {
            return Err(TelemetryError::InvalidTargetForMetric);
        }
        Ok(())
    }
}
