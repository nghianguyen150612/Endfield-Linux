use endfield_unix_domain::MeasurementUnit;

use crate::errors::TelemetryError;
use crate::units::TelemetryUnit;

/// Stable IDs in the telemetry schema's private 16-bit namespace.
///
/// These values are independent of P007 capability IDs and the P003 C ABI.
/// Once released, an assigned value is never reused for a different metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum MetricId {
    CpuUtilizationWholeMachine = 0x0101,
    CpuUtilizationLogicalCpu = 0x0102,
    CpuFrequency = 0x0103,
    ProcessCpuTime = 0x0104,
    CpuTemperature = 0x0105,
    CpuPower = 0x0106,
    ProcessCpuUtilization = 0x0107,
    RamUsed = 0x0201,
    RamAvailable = 0x0202,
    SwapUsed = 0x0203,
    ProcessMemoryUsed = 0x0204,
    CgroupMemoryUsed = 0x0205,
    GpuUtilization = 0x0301,
    GpuMemoryUsed = 0x0302,
    GpuDedicatedMemoryCapacity = 0x0303,
    GpuTemperature = 0x0304,
    GpuPower = 0x0305,
    GpuFrequency = 0x0306,
    GpuEngineUtilization = 0x0307,
    FramesPerSecond = 0x0401,
    FrameTime = 0x0402,
    MeanFrameTime = 0x0403,
    FrameCount = 0x0404,
    P99FrameTime = 0x0405,
}

impl MetricId {
    pub const fn stable_id(self) -> u16 {
        self as u16
    }

    pub const fn definition(self) -> &'static MetricDefinition {
        match self {
            Self::CpuUtilizationWholeMachine => &METRIC_REGISTRY[0],
            Self::CpuUtilizationLogicalCpu => &METRIC_REGISTRY[1],
            Self::CpuFrequency => &METRIC_REGISTRY[2],
            Self::ProcessCpuTime => &METRIC_REGISTRY[3],
            Self::CpuTemperature => &METRIC_REGISTRY[4],
            Self::CpuPower => &METRIC_REGISTRY[5],
            Self::ProcessCpuUtilization => &METRIC_REGISTRY[6],
            Self::RamUsed => &METRIC_REGISTRY[7],
            Self::RamAvailable => &METRIC_REGISTRY[8],
            Self::SwapUsed => &METRIC_REGISTRY[9],
            Self::ProcessMemoryUsed => &METRIC_REGISTRY[10],
            Self::CgroupMemoryUsed => &METRIC_REGISTRY[11],
            Self::GpuUtilization => &METRIC_REGISTRY[12],
            Self::GpuMemoryUsed => &METRIC_REGISTRY[13],
            Self::GpuDedicatedMemoryCapacity => &METRIC_REGISTRY[14],
            Self::GpuTemperature => &METRIC_REGISTRY[15],
            Self::GpuPower => &METRIC_REGISTRY[16],
            Self::GpuFrequency => &METRIC_REGISTRY[17],
            Self::GpuEngineUtilization => &METRIC_REGISTRY[18],
            Self::FramesPerSecond => &METRIC_REGISTRY[19],
            Self::FrameTime => &METRIC_REGISTRY[20],
            Self::MeanFrameTime => &METRIC_REGISTRY[21],
            Self::FrameCount => &METRIC_REGISTRY[22],
            Self::P99FrameTime => &METRIC_REGISTRY[23],
        }
    }
}

impl TryFrom<u16> for MetricId {
    type Error = TelemetryError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        METRIC_REGISTRY
            .iter()
            .find(|definition| definition.id.stable_id() == value)
            .map(|definition| definition.id)
            .ok_or(TelemetryError::UnknownMetricId(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricScope {
    Platform,
    Cpu,
    LogicalCpu,
    Process,
    Cgroup,
    Gpu,
    GpuEngine,
    GameSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    Gauge,
    Counter,
    Duration,
    Rate,
    DerivedStatistic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricValueKind {
    Unsigned,
    Signed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericRange {
    Unsigned { minimum: u64, maximum: Option<u64> },
    Signed { minimum: i64, maximum: i64 },
}

impl NumericRange {
    pub(crate) fn accepts(self, value: endfield_unix_domain::MeasurementValue) -> bool {
        use endfield_unix_domain::MeasurementValue;
        match (self, value) {
            (Self::Unsigned { minimum, maximum }, MeasurementValue::Unsigned(value)) => {
                value >= minimum && maximum.is_none_or(|maximum| value <= maximum)
            }
            (Self::Signed { minimum, maximum }, MeasurementValue::Signed(value)) => {
                value >= minimum && value <= maximum
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricDefinition {
    pub id: MetricId,
    pub name: &'static str,
    pub description: &'static str,
    pub unit: TelemetryUnit,
    pub value_kind: MetricValueKind,
    pub permitted_range: Option<NumericRange>,
    pub scope: MetricScope,
    pub kind: MetricKind,
}

const fn domain_unit(unit: MeasurementUnit) -> TelemetryUnit {
    TelemetryUnit::from_domain_const(unit)
}

const UTILIZATION_RANGE: NumericRange = NumericRange::Unsigned {
    minimum: 0,
    maximum: Some(10_000),
};

static METRIC_REGISTRY: [MetricDefinition; 24] = [
    MetricDefinition {
        id: MetricId::CpuUtilizationWholeMachine,
        name: "cpu.utilization.whole_machine",
        description: "CPU busy time normalized to the complete logical-processor capacity",
        unit: TelemetryUnit::BasisPoints,
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(UTILIZATION_RANGE),
        scope: MetricScope::Platform,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::CpuUtilizationLogicalCpu,
        name: "cpu.utilization.logical_cpu",
        description: "CPU busy time normalized to one logical CPU",
        unit: TelemetryUnit::BasisPoints,
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(UTILIZATION_RANGE),
        scope: MetricScope::LogicalCpu,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::CpuFrequency,
        name: "cpu.frequency",
        description: "Observed CPU frequency",
        unit: domain_unit(MeasurementUnit::Hertz),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Cpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::ProcessCpuTime,
        name: "process.cpu_time",
        description: "Cumulative CPU execution time for an explicitly scoped process",
        unit: domain_unit(MeasurementUnit::Nanoseconds),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Process,
        kind: MetricKind::Counter,
    },
    MetricDefinition {
        id: MetricId::CpuTemperature,
        name: "cpu.temperature",
        description: "Observed CPU temperature, including valid signed milli-Celsius values",
        unit: domain_unit(MeasurementUnit::MilliCelsius),
        value_kind: MetricValueKind::Signed,
        permitted_range: None,
        scope: MetricScope::Cpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::CpuPower,
        name: "cpu.power",
        description: "Observed CPU power",
        unit: domain_unit(MeasurementUnit::Milliwatts),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Cpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::ProcessCpuUtilization,
        name: "process.cpu_utilization",
        description: "Process CPU use normalized to one logical CPU and therefore allowed to exceed 100 percent",
        unit: TelemetryUnit::BasisPoints,
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Process,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::RamUsed,
        name: "memory.ram_used",
        description: "Observed host RAM in use",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Platform,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::RamAvailable,
        name: "memory.ram_available",
        description: "Observed host RAM available to workloads",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Platform,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::SwapUsed,
        name: "memory.swap_used",
        description: "Observed host swap in use",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Platform,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::ProcessMemoryUsed,
        name: "process.memory_used",
        description: "Observed resident or explicitly documented process memory",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Process,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::CgroupMemoryUsed,
        name: "cgroup.memory_used",
        description: "Observed cgroup memory usage for a known accounting scope",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Cgroup,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuUtilization,
        name: "gpu.utilization.total_device",
        description: "Total-device GPU utilization; engine-specific readings require a separately defined metric",
        unit: TelemetryUnit::BasisPoints,
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(UTILIZATION_RANGE),
        scope: MetricScope::Gpu,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::GpuMemoryUsed,
        name: "gpu.memory_used",
        description: "Observed GPU memory usage",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Gpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuDedicatedMemoryCapacity,
        name: "gpu.dedicated_memory_capacity",
        description: "Verified dedicated GPU memory capacity when a provider can establish it",
        unit: domain_unit(MeasurementUnit::Bytes),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Gpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuTemperature,
        name: "gpu.temperature",
        description: "Observed GPU temperature",
        unit: domain_unit(MeasurementUnit::MilliCelsius),
        value_kind: MetricValueKind::Signed,
        permitted_range: None,
        scope: MetricScope::Gpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuPower,
        name: "gpu.power",
        description: "Observed GPU power",
        unit: domain_unit(MeasurementUnit::Milliwatts),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Gpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuFrequency,
        name: "gpu.frequency",
        description: "Observed GPU frequency where the provider supplies it",
        unit: domain_unit(MeasurementUnit::Hertz),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::Gpu,
        kind: MetricKind::Gauge,
    },
    MetricDefinition {
        id: MetricId::GpuEngineUtilization,
        name: "gpu.engine_utilization",
        description: "Utilization of one explicitly identified GPU engine",
        unit: TelemetryUnit::BasisPoints,
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(UTILIZATION_RANGE),
        scope: MetricScope::GpuEngine,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::FramesPerSecond,
        name: "frame.frames_per_second",
        description: "Frame rate represented in milli-frames per second over a documented interval",
        unit: domain_unit(MeasurementUnit::MilliFramesPerSecond),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::GameSession,
        kind: MetricKind::Rate,
    },
    MetricDefinition {
        id: MetricId::FrameTime,
        name: "frame.duration",
        description: "One observed frame duration",
        unit: domain_unit(MeasurementUnit::Nanoseconds),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(NumericRange::Unsigned {
            minimum: 1,
            maximum: None,
        }),
        scope: MetricScope::GameSession,
        kind: MetricKind::Duration,
    },
    MetricDefinition {
        id: MetricId::MeanFrameTime,
        name: "frame.mean_duration",
        description: "Mean frametime computed from multiple documented frame observations",
        unit: domain_unit(MeasurementUnit::Nanoseconds),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(NumericRange::Unsigned {
            minimum: 1,
            maximum: None,
        }),
        scope: MetricScope::GameSession,
        kind: MetricKind::DerivedStatistic,
    },
    MetricDefinition {
        id: MetricId::FrameCount,
        name: "frame.count",
        description: "Cumulative frame count within a known game-session lifetime",
        unit: domain_unit(MeasurementUnit::Count),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: None,
        scope: MetricScope::GameSession,
        kind: MetricKind::Counter,
    },
    MetricDefinition {
        id: MetricId::P99FrameTime,
        name: "frame.p99_duration",
        description: "99th-percentile frametime from multiple documented frame observations",
        unit: domain_unit(MeasurementUnit::Nanoseconds),
        value_kind: MetricValueKind::Unsigned,
        permitted_range: Some(NumericRange::Unsigned {
            minimum: 1,
            maximum: None,
        }),
        scope: MetricScope::GameSession,
        kind: MetricKind::DerivedStatistic,
    },
];

/// Returns the entire stable in-memory registry in ascending ID order.
pub fn metric_registry() -> &'static [MetricDefinition] {
    &METRIC_REGISTRY
}
