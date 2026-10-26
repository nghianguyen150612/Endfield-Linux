//! PCI-only GPU identity and one-shot P015 provider. Never initializes Vulkan.
use crate::gpu_nvidia::NvidiaBackend;
use crate::gpu_telemetry_sources::*;
use crate::{
    LinuxGpuDataSource, LinuxGpuReport, LinuxTelemetryClockFactory, MonotonicClockSession, PciBdf,
    PciGraphicsDevice, TelemetryClockFactory,
};
use endfield_unix_domain::{
    Availability, CapabilityValue, GpuClass, GpuInfo, GraphicsDriver, MeasurementOrigin,
    MeasurementValue, SystemSnapshot,
};
use endfield_unix_telemetry::*;

pub const GPU_METRICS: [MetricId; 6] = [
    MetricId::GpuUtilization,
    MetricId::GpuMemoryUsed,
    MetricId::GpuDedicatedMemoryCapacity,
    MetricId::GpuTemperature,
    MetricId::GpuPower,
    MetricId::GpuFrequency,
];
#[derive(Clone, Debug)]
pub struct GpuIdentityContext {
    snapshot: SnapshotContext,
    devices: Vec<PciGraphicsDevice>,
    discrete: Vec<bool>,
}
impl GpuIdentityContext {
    /// Reuses P013's parser and deterministic sorted-BDF ID allocation. The
    /// supplied base snapshot must not already contain GPUs.
    pub fn discover(
        source: &impl LinuxGpuDataSource,
        mut base: SystemSnapshot,
        reference: SnapshotRefId,
    ) -> Result<Self, SensorError> {
        if !base.gpus.is_empty() {
            return Err(SensorError::Ambiguous);
        }
        let mut entries = source
            .enumerate_pci_entries()
            .map_err(|_| SensorError::Temporary)?;
        if entries.len() > crate::MAX_PCI_DEVICES {
            return Err(SensorError::Malformed);
        }
        entries.sort();
        if entries.windows(2).any(|p| p[0] == p[1]) {
            return Err(SensorError::Ambiguous);
        }
        let mut devices = Vec::new();
        for entry in entries {
            let bdf: PciBdf = entry.parse().map_err(|_| SensorError::Malformed)?;
            if let Some(device) = crate::gpu_probe::read_pci_device(source, bdf.to_string())
                .map_err(|_| SensorError::Malformed)?
            {
                devices.push(device);
            }
        }
        devices.sort_by(|a, b| a.bdf.cmp(&b.bdf));
        if devices.windows(2).any(|p| p[0].bdf == p[1].bdf) {
            return Err(SensorError::Ambiguous);
        }
        // Bound the full six-metric request before any sensor access.
        if devices.len() > MAX_OBSERVATIONS_PER_BATCH / GPU_METRICS.len() {
            return Err(SensorError::Malformed);
        }
        let provenance_id = endfield_unix_domain::ProvenanceId::new(
            base.provenance
                .iter()
                .map(|p| p.id.get())
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(SensorError::Malformed)?,
        )
        .map_err(|_| SensorError::Malformed)?;
        base.provenance.push(
            endfield_unix_domain::Provenance::new(provenance_id, "linux-pci".to_owned(), None)
                .map_err(|_| SensorError::Malformed)?,
        );
        let ids = crate::gpu_probe::allocate_gpu_ids(devices.len())
            .map_err(|_| SensorError::Malformed)?;
        for (device, instance) in devices.iter().zip(ids) {
            use endfield_unix_domain::{CapabilityId, CapabilityRecord};
            for (id, availability) in [
                (CapabilityId::GpuDevice, Availability::Available),
                (CapabilityId::GraphicsDriver, device.driver_binding),
                (CapabilityId::Vram, Availability::Unknown),
                (CapabilityId::HardwareAcceleration, Availability::Unknown),
                (CapabilityId::VulkanDevice, Availability::Unknown),
            ] {
                let mut record = CapabilityRecord::new(id, availability);
                record.instance = Some(instance);
                record.provenance = Some(provenance_id);
                base.capabilities.push(record);
            }
            base.gpus.push(GpuInfo {
                instance,
                availability: Availability::Available,
                vendor: Some(crate::gpu_probe::gpu_vendor(device.vendor_id)),
                model: None,
                class: GpuClass::Unknown,
                graphics_driver: device.driver_name.as_ref().map(|name| GraphicsDriver {
                    name: name.clone(),
                    version: None,
                }),
                driver_availability: device.driver_binding,
                vram: CapabilityValue::new(Availability::Unknown, None),
                hardware_acceleration: Availability::Unknown,
                vulkan_device: Availability::Unknown,
            });
        }
        let snapshot =
            SnapshotContext::from_snapshot(reference, &base).map_err(|_| SensorError::Malformed)?;
        let discrete = vec![false; devices.len()];
        Ok(Self {
            snapshot,
            devices,
            discrete,
        })
    }
    pub fn from_report(
        report: &LinuxGpuReport,
        reference: SnapshotRefId,
    ) -> Result<Self, SensorError> {
        let devices = &report.pci_devices;
        if report.schema_version != crate::LINUX_GPU_REPORT_SCHEMA_VERSION
            || devices.len() != report.snapshot.gpus.len()
            || devices.len() > MAX_OBSERVATIONS_PER_BATCH / 6
            || devices.windows(2).any(|p| p[0].bdf >= p[1].bdf)
        {
            return Err(SensorError::Ambiguous);
        }
        let ids = crate::gpu_probe::allocate_gpu_ids(devices.len())
            .map_err(|_| SensorError::Malformed)?;
        for ((d, g), id) in devices.iter().zip(&report.snapshot.gpus).zip(ids) {
            if d.bdf.parse::<PciBdf>().is_err()
                || d.bdf
                    .parse::<PciBdf>()
                    .is_ok_and(|b| b.to_string() != d.bdf)
                || !PciBdf::is_display_controller_class(d.class_code)
                || g.driver_availability != d.driver_binding
                || g.instance != id
                || g.vendor != Some(crate::gpu_probe::gpu_vendor(d.vendor_id))
                || g.graphics_driver.as_ref().map(|v| &v.name) != d.driver_name.as_ref()
            {
                return Err(SensorError::Ambiguous);
            }
        }
        Ok(Self {
            snapshot: SnapshotContext::from_snapshot(reference, &report.snapshot)
                .map_err(|_| SensorError::Malformed)?,
            devices: devices.clone(),
            discrete: report
                .snapshot
                .gpus
                .iter()
                .map(|g| g.class == GpuClass::Discrete)
                .collect(),
        })
    }
    pub fn devices(&self) -> &[PciGraphicsDevice] {
        &self.devices
    }
    pub fn target(&self, index: usize) -> Option<ObservationTarget> {
        self.devices.get(index)?;
        let id = crate::gpu_probe::allocate_gpu_ids(self.devices.len()).ok()?[index];
        Some(ObservationTarget::Gpu(DeviceReference::new(
            self.snapshot.reference(),
            id,
        )))
    }
}
pub struct LinuxGpuTelemetryProvider<S, N, C = LinuxTelemetryClockFactory> {
    source: S,
    nvidia: N,
    context: Option<GpuIdentityContext>,
    clock: C,
    id: ProviderId,
}
impl<S, N> LinuxGpuTelemetryProvider<S, N> {
    pub fn new(source: S, nvidia: N, context: Option<GpuIdentityContext>) -> Self {
        Self::with_clock_factory(source, nvidia, context, LinuxTelemetryClockFactory)
    }
}
impl<S, N, C> LinuxGpuTelemetryProvider<S, N, C> {
    pub fn with_clock_factory(
        source: S,
        nvidia: N,
        context: Option<GpuIdentityContext>,
        clock: C,
    ) -> Self {
        Self {
            source,
            nvidia,
            context,
            clock,
            id: ProviderId::new("linux-gpu-telemetry").expect("static provider ID"),
        }
    }
    pub fn context(&self) -> Option<&GpuIdentityContext> {
        self.context.as_ref()
    }
}
fn failure(error: SensorError) -> (ObservationState, ProviderIssueKind) {
    match error {
        SensorError::Missing => (
            ObservationState::Unknown,
            ProviderIssueKind::MissingProvider,
        ),
        SensorError::Unsupported => (
            ObservationState::Unsupported,
            ProviderIssueKind::Unsupported,
        ),
        SensorError::Denied => (
            ObservationState::PermissionDenied,
            ProviderIssueKind::PermissionDenied,
        ),
        SensorError::Temporary => (
            ObservationState::TemporaryFailure,
            ProviderIssueKind::TemporaryFailure,
        ),
        SensorError::Malformed => (
            ObservationState::MalformedSourceData,
            ProviderIssueKind::MalformedSourceData,
        ),
        SensorError::Ambiguous | SensorError::Suspended => (
            ObservationState::Degraded(None),
            ProviderIssueKind::Degraded,
        ),
    }
}
impl<S: GpuSensorSource + LinuxGpuDataSource, N: NvidiaBackend, C: TelemetryClockFactory>
    TelemetryProvider for LinuxGpuTelemetryProvider<S, N, C>
{
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn declared_metrics(&self) -> &[MetricId] {
        &GPU_METRICS
    }
    fn collect(&self, request: &CollectionRequest) -> Result<ProviderCollection, ProviderError> {
        let fatal = || ProviderError::new(self.id.clone(), ProviderErrorKind::MalformedSourceData);
        let mut clock = self.clock.start_session().map_err(|_| {
            ProviderError::new(self.id.clone(), ProviderErrorKind::TemporaryFailure)
        })?;
        let mut observations = Vec::new();
        let mut issues = Vec::new();
        let requested: Vec<_> = request
            .metrics()
            .iter()
            .copied()
            .filter(|m| GPU_METRICS.contains(m))
            .collect();
        if requested.is_empty() {
            return ProviderCollection::complete(
                TelemetryBatch::new(request.context(), None, None, Vec::new())
                    .map_err(|_| fatal())?,
            )
            .map_err(|_| fatal());
        }
        if let Some(context) = &self.context {
            // NVML initialization itself may wake GPUs. Skip the entire library
            // unless every discovered NVIDIA device is verified active.
            let identity_current = |d: &PciGraphicsDevice| {
                crate::gpu_probe::read_pci_device(&self.source, d.bdf.clone())
                    .ok()
                    .flatten()
                    .is_some_and(|current| {
                        current.vendor_id == d.vendor_id
                            && current.device_id == d.device_id
                            && current.driver_name == d.driver_name
                    })
            };
            let native_started = clock.now().map_err(|_| fatal())?;
            let allow_nvml = context
                .devices
                .iter()
                .filter(|d| d.vendor_id == 0x10de)
                .all(|d| {
                    self.source.power(d.bdf.parse().unwrap()) == RuntimePower::Active
                        && identity_current(d)
                });
            let native = if allow_nvml
                && context
                    .devices
                    .iter()
                    .any(|d| d.vendor_id == 0x10de && d.driver_name.as_deref() == Some("nvidia"))
                && requested.iter().any(|m| *m != MetricId::GpuUtilization)
            {
                Some(
                    self.nvidia.collect(
                        &context
                            .devices
                            .iter()
                            .filter(|d| {
                                d.vendor_id == 0x10de && d.driver_name.as_deref() == Some("nvidia")
                            })
                            .map(|d| d.bdf.parse().unwrap())
                            .collect::<Vec<_>>(),
                        &requested,
                    ),
                )
            } else {
                None
            };
            for (index, device) in context.devices.iter().enumerate() {
                let bdf: PciBdf = device.bdf.parse().map_err(|_| fatal())?;
                let power = self.source.power(bdf);
                let driver = device.driver_name.as_deref().unwrap_or("");
                let current = power == RuntimePower::Active && identity_current(device);
                for &metric in &requested {
                    let before = clock.now().map_err(|_| fatal())?;
                    let result = if power != RuntimePower::Active {
                        Err(SensorError::Suspended)
                    } else if !current {
                        Err(SensorError::Ambiguous)
                    } else if metric == MetricId::GpuUtilization {
                        Err(SensorError::Unsupported)
                    } else if device.vendor_id == 0x10de && driver == "nvidia" {
                        native
                            .as_ref()
                            .ok_or(SensorError::Suspended)
                            .and_then(|r| r.as_ref().map_err(|e| *e))
                            .and_then(|readings| {
                                let matches: Vec<_> =
                                    readings.iter().filter(|r| r.bdf == bdf).collect();
                                if matches.len() != 1 {
                                    return Err(SensorError::Ambiguous);
                                }
                                matches[0]
                                    .values
                                    .iter()
                                    .find(|(m, _)| *m == metric)
                                    .map(|(_, v)| *v)
                                    .unwrap_or(Err(SensorError::Unsupported))
                            })
                    } else {
                        read_sysfs(
                            &self.source,
                            bdf,
                            device.vendor_id,
                            driver,
                            metric,
                            context.discrete[index],
                        )
                    };
                    let (state, quality) = match result {
                        Ok(value) => (
                            ObservationState::Measured(TelemetryMeasurement::new(
                                value,
                                metric.definition().unit,
                            )),
                            QualityMetadata::complete(),
                        ),
                        Err(e) => {
                            let (state, kind) = failure(e);
                            issues.push(ProviderIssue::new(metric, kind));
                            (state, QualityMetadata::partial())
                        }
                    };
                    let after = clock.now().map_err(|_| fatal())?;
                    let name = if device.vendor_id == 0x10de {
                        "linux-nvidia-nvml"
                    } else if driver == "amdgpu" {
                        "linux-amdgpu-sysfs"
                    } else if device.vendor_id == 0x8086 {
                        "linux-intel-sysfs"
                    } else {
                        "linux-gpu-unsupported"
                    };
                    observations.push(
                        TelemetryObservation::new(
                            metric,
                            context.target(index).ok_or_else(fatal)?,
                            state,
                            TemporalMetadata::new(
                                ObservationTime::monotonic(if device.vendor_id == 0x10de {
                                    native_started
                                } else {
                                    before
                                }),
                                Some(after),
                                None,
                            )
                            .map_err(|_| fatal())?,
                            TelemetryProvenance::new(
                                ProviderId::new(name).map_err(|_| fatal())?,
                                SamplingMethod::OperatingSystemInterface,
                                MeasurementOrigin::Normalized,
                            )
                            .map_err(|_| fatal())?,
                            quality,
                            None,
                        )
                        .map_err(|_| fatal())?,
                    );
                }
            }
            if context.devices.is_empty() && !requested.is_empty() {
                issues.push(ProviderIssue::new(
                    requested[0],
                    ProviderIssueKind::MissingProvider,
                ));
            }
        } else if !requested.is_empty() {
            issues.push(ProviderIssue::new(
                requested[0],
                ProviderIssueKind::NotYetSampled,
            ));
        }
        let batch = TelemetryBatch::new(
            request.context(),
            Some(clock.domain()),
            self.context.as_ref().map(|c| c.snapshot.clone()),
            observations,
        )
        .map_err(|_| fatal())?;
        if issues.is_empty() {
            ProviderCollection::complete(batch)
        } else {
            ProviderCollection::partial(batch, issues)
        }
        .map_err(|_| fatal())
    }
}
fn read_sysfs(
    source: &impl GpuSensorSource,
    bdf: PciBdf,
    vendor: u16,
    driver: &str,
    metric: MetricId,
    discrete: bool,
) -> Result<MeasurementValue, SensorError> {
    use MeasurementValue::{Signed, Unsigned};
    if vendor == 0x1002 && driver == "amdgpu" {
        match metric {
            MetricId::GpuMemoryUsed | MetricId::GpuDedicatedMemoryCapacity => {
                if metric == MetricId::GpuDedicatedMemoryCapacity && !discrete {
                    return Err(SensorError::Unsupported);
                }
                let total = unsigned(&source.attribute(bdf, GpuAttribute::VramTotal)?)?;
                if total == 0 {
                    return Err(SensorError::Unsupported);
                }
                if metric == MetricId::GpuDedicatedMemoryCapacity {
                    return Ok(Unsigned(total));
                }
                let used = unsigned(&source.attribute(bdf, GpuAttribute::VramUsed)?)?;
                if used > total {
                    return Err(SensorError::Malformed);
                }
                Ok(Unsigned(used))
            }
            MetricId::GpuTemperature => Ok(Signed(signed(&source.hwmon(bdf, driver, metric)?)?)),
            MetricId::GpuPower => Ok(Unsigned(microwatts_to_milliwatts(unsigned(
                &source.hwmon(bdf, driver, metric)?,
            )?))),
            _ => Err(SensorError::Unsupported),
        }
    } else if vendor == 0x8086 && driver == "i915" && metric == MetricId::GpuFrequency {
        Ok(Unsigned(mhz_to_hz(unsigned(
            &source.attribute(bdf, GpuAttribute::IntelCurrentMhz)?,
        )?)?))
    } else {
        Err(SensorError::Unsupported)
    }
}
