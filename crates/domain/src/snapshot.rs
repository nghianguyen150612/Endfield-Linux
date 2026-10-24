use std::collections::{BTreeMap, BTreeSet};

use crate::capability::{
    Availability, CapabilityId, CapabilityRecord, InstanceId, MeasurementUnit, MeasurementValue,
};
use crate::display::DisplayInfo;
use crate::hardware::{CpuInfo, GpuInfo, MemoryInfo};
use crate::platform::PlatformIdentity;
use crate::provenance::{Diagnostic, Provenance};
use crate::validation::{ValidationError, ValidationErrors};

pub const CURRENT_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSnapshot {
    /// Version of this in-memory domain contract, independent of app, storage,
    /// and C ABI versions.
    pub schema_version: u32,
    pub platform: PlatformIdentity,
    pub cpu: Option<CpuInfo>,
    pub gpus: Vec<GpuInfo>,
    pub memory: Option<MemoryInfo>,
    pub displays: Vec<DisplayInfo>,
    pub capabilities: Vec<CapabilityRecord>,
    pub provenance: Vec<Provenance>,
    pub diagnostics: Vec<Diagnostic>,
}

impl SystemSnapshot {
    pub fn new(platform: PlatformIdentity) -> Self {
        Self {
            schema_version: CURRENT_SNAPSHOT_SCHEMA_VERSION,
            platform,
            cpu: None,
            gpus: Vec::new(),
            memory: None,
            displays: Vec::new(),
            capabilities: Vec::new(),
            provenance: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        if self.schema_version != CURRENT_SNAPSHOT_SCHEMA_VERSION {
            errors.push(ValidationError::UnsupportedSnapshotSchema {
                found: self.schema_version,
                supported: CURRENT_SNAPSHOT_SCHEMA_VERSION,
            });
        }
        if let Err(error) = self.platform.validate() {
            errors.push(error);
        }

        let mut instances = BTreeSet::new();
        if let Some(cpu) = &self.cpu {
            insert_instance(&mut instances, cpu.instance, &mut errors);
            if let Err(error) = cpu.validate() {
                errors.push(error);
            }
        }
        for gpu in &self.gpus {
            insert_instance(&mut instances, gpu.instance, &mut errors);
            if let Err(error) = gpu.validate() {
                errors.push(error);
            }
        }
        for display in &self.displays {
            insert_instance(&mut instances, display.instance, &mut errors);
            if let Err(error) = display.validate() {
                errors.push(error);
            }
        }
        if let Some(memory) = &self.memory {
            if let Err(error) = memory.validate() {
                errors.push(error);
            }
        }

        let mut provenance_ids = BTreeSet::new();
        for item in &self.provenance {
            if !provenance_ids.insert(item.id.get()) {
                errors.push(ValidationError::DuplicateProvenance {
                    provenance_id: item.id.get(),
                });
            }
            if let Err(error) = item.validate() {
                errors.push(error);
            }
        }

        let mut capability_keys = BTreeSet::new();
        let mut capability_map = BTreeMap::new();
        for capability in &self.capabilities {
            let instance_id = capability.instance.map(InstanceId::get);
            if !capability_keys.insert((capability.id.abi_id(), instance_id)) {
                errors.push(ValidationError::DuplicateCapability {
                    capability_id: capability.id.abi_id(),
                    instance_id,
                });
            }
            if let Some(instance_id) = instance_id {
                if !instances.contains(&instance_id) {
                    errors.push(ValidationError::UnknownInstanceReference { instance_id });
                }
            }
            if let Some(provenance) = capability.provenance {
                if !provenance_ids.contains(&provenance.get()) {
                    errors.push(ValidationError::UnknownProvenanceReference {
                        provenance_id: provenance.get(),
                    });
                }
            }
            if let Err(error) = capability.validate() {
                errors.push(error);
            }
            capability_map.insert((capability.id.abi_id(), instance_id), capability);
        }

        if let Some(cpu) = &self.cpu {
            let instance = Some(cpu.instance);
            let topology_measurement = cpu.topology.value.as_ref().map(|topology| {
                (
                    MeasurementUnit::Count,
                    MeasurementValue::Unsigned(u64::from(topology.logical_processor_count)),
                )
            });
            check_typed_capability(
                &capability_map,
                CapabilityId::CpuTopology,
                instance,
                cpu.topology.availability,
                Some(topology_measurement),
                &mut errors,
            );
            check_typed_capability(
                &capability_map,
                CapabilityId::CpuScheduling,
                instance,
                cpu.scheduling,
                Some(None),
                &mut errors,
            );
            let frequency_power =
                combine_availability(cpu.observed_frequency.availability, cpu.power_management);
            let frequency_measurement = cpu.observed_frequency.value.map(|frequency| {
                (
                    MeasurementUnit::Hertz,
                    MeasurementValue::Unsigned(frequency.0),
                )
            });
            check_typed_capability(
                &capability_map,
                CapabilityId::CpuFrequencyPower,
                instance,
                frequency_power,
                frequency_measurement.map(Some),
                &mut errors,
            );
        }

        for gpu in &self.gpus {
            let instance = Some(gpu.instance);
            check_typed_capability(
                &capability_map,
                CapabilityId::GpuDevice,
                instance,
                gpu.availability,
                None,
                &mut errors,
            );
            let vram_measurement = gpu
                .vram
                .value
                .map(|bytes| (MeasurementUnit::Bytes, MeasurementValue::Unsigned(bytes.0)));
            check_typed_capability(
                &capability_map,
                CapabilityId::Vram,
                instance,
                gpu.vram.availability,
                Some(vram_measurement),
                &mut errors,
            );
            check_typed_capability(
                &capability_map,
                CapabilityId::HardwareAcceleration,
                instance,
                gpu.hardware_acceleration,
                Some(None),
                &mut errors,
            );
            check_typed_capability(
                &capability_map,
                CapabilityId::GraphicsDriver,
                instance,
                gpu.driver_availability,
                Some(None),
                &mut errors,
            );
            check_typed_capability(
                &capability_map,
                CapabilityId::VulkanDevice,
                instance,
                gpu.vulkan_device,
                Some(None),
                &mut errors,
            );
        }

        if let Some(memory) = &self.memory {
            let total_ram_measurement = memory.ram.value.as_ref().and_then(|capacity| {
                capacity
                    .total
                    .map(|bytes| (MeasurementUnit::Bytes, MeasurementValue::Unsigned(bytes.0)))
            });
            check_typed_capability(
                &capability_map,
                CapabilityId::Ram,
                None,
                memory.ram.availability,
                Some(total_ram_measurement),
                &mut errors,
            );
            let swap_measurement = memory
                .swap
                .total
                .map(|bytes| (MeasurementUnit::Bytes, MeasurementValue::Unsigned(bytes.0)));
            check_typed_capability(
                &capability_map,
                CapabilityId::Swap,
                None,
                memory.swap.availability,
                Some(swap_measurement),
                &mut errors,
            );
        }

        for display in &self.displays {
            check_typed_capability(
                &capability_map,
                CapabilityId::Display,
                Some(display.instance),
                display.availability,
                None,
                &mut errors,
            );
            check_typed_capability(
                &capability_map,
                CapabilityId::CompositorSession,
                Some(display.instance),
                display.session_availability,
                Some(None),
                &mut errors,
            );
        }

        for diagnostic in &self.diagnostics {
            if let Err(error) = diagnostic.validate() {
                errors.push(error);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors(errors))
        }
    }

    /// Validates and canonicalizes records whose order has no semantic meaning.
    pub fn normalize(mut self) -> Result<Self, ValidationErrors> {
        self.validate()?;
        self.gpus.sort_by_key(|gpu| gpu.instance);
        self.displays.sort_by_key(|display| display.instance);
        self.capabilities.sort_by_key(|capability| {
            (
                capability.id.abi_id(),
                capability.instance.map(InstanceId::get),
            )
        });
        self.provenance.sort_by_key(|item| item.id);
        self.diagnostics.sort_by(|left, right| {
            (left.category, &left.provider, &left.message).cmp(&(
                right.category,
                &right.provider,
                &right.message,
            ))
        });
        Ok(self)
    }

    pub(crate) fn append_report(&mut self, report: crate::interfaces::ProbeReport) {
        if report.cpu.is_some() {
            self.cpu = report.cpu;
        }
        self.gpus.extend(report.gpus);
        if report.memory.is_some() {
            self.memory = report.memory;
        }
        self.displays.extend(report.displays);
        self.capabilities.extend(report.capabilities);
        self.provenance.extend(report.provenance);
        self.diagnostics.extend(report.diagnostics);
    }
}

fn check_typed_capability(
    capabilities: &BTreeMap<(u32, Option<u32>), &CapabilityRecord>,
    id: CapabilityId,
    instance: Option<InstanceId>,
    availability: Availability,
    expected_measurement: Option<Option<(MeasurementUnit, MeasurementValue)>>,
    errors: &mut Vec<ValidationError>,
) {
    let instance_id = instance.map(InstanceId::get);
    let key = (id.abi_id(), instance_id);
    match capabilities.get(&key) {
        Some(record) => {
            if record.availability != availability {
                errors.push(ValidationError::CapabilityAvailabilityMismatch {
                    capability_id: id.abi_id(),
                    instance_id,
                });
            }
            if let Some(expected) = expected_measurement {
                let matches = match (record.measurement, expected) {
                    (None, None) => true,
                    (Some(actual), Some((unit, value))) => {
                        actual.unit() == unit && actual.value() == value
                    }
                    _ => false,
                };
                if !matches {
                    errors.push(ValidationError::CapabilityMeasurementMismatch {
                        capability_id: id.abi_id(),
                        instance_id,
                    });
                }
            }
        }
        None if availability != Availability::Unknown => {
            errors.push(ValidationError::MissingCapabilityRecord {
                capability_id: id.abi_id(),
                instance_id,
            });
        }
        None => {}
    }
}

fn combine_availability(left: Availability, right: Availability) -> Availability {
    if left == right {
        left
    } else {
        Availability::Degraded
    }
}

fn insert_instance(
    instances: &mut BTreeSet<u32>,
    instance: InstanceId,
    errors: &mut Vec<ValidationError>,
) {
    if !instances.insert(instance.get()) {
        errors.push(ValidationError::DuplicateInstance {
            instance_id: instance.get(),
        });
    }
}
