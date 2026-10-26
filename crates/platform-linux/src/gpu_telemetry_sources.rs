//! Fixed read-only GPU sensor families. No caller-controlled filesystem paths.
use crate::gpu_sources::{map_io_error, read_attribute};
use crate::{DataSourceErrorKind, PciBdf, SystemLinuxDataSource};
use endfield_unix_telemetry::MetricId;
use std::{fs, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorError {
    Missing,
    Unsupported,
    Denied,
    Suspended,
    Temporary,
    Malformed,
    Ambiguous,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePower {
    Active,
    Suspended,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuAttribute {
    VramUsed,
    VramTotal,
    BusyPercent,
    IntelCurrentMhz,
}
impl GpuAttribute {
    fn name(self) -> &'static str {
        match self {
            Self::VramUsed => "mem_info_vram_used",
            Self::VramTotal => "mem_info_vram_total",
            Self::BusyPercent => "gpu_busy_percent",
            Self::IntelCurrentMhz => "gt_act_freq_mhz",
        }
    }
}
pub trait GpuSensorSource: Send + Sync {
    fn power(&self, bdf: PciBdf) -> RuntimePower;
    fn attribute(&self, bdf: PciBdf, attribute: GpuAttribute) -> Result<Vec<u8>, SensorError>;
    /// Values are signed milli-Celsius or unsigned microwatts. Implementations
    /// must establish sensor semantics and reject multiple eligible sensors.
    fn hwmon(&self, bdf: PciBdf, driver: &str, metric: MetricId) -> Result<Vec<u8>, SensorError>;
}
fn error(e: crate::DataSourceError) -> SensorError {
    match e.kind {
        DataSourceErrorKind::NotFound => SensorError::Missing,
        DataSourceErrorKind::PermissionDenied => SensorError::Denied,
        DataSourceErrorKind::Other => SensorError::Temporary,
        _ => SensorError::Malformed,
    }
}
fn contained_read(root: &std::path::Path, path: &std::path::Path) -> Result<Vec<u8>, SensorError> {
    let resolved = fs::canonicalize(path)
        .map_err(map_io_error)
        .map_err(error)?;
    if !resolved.starts_with(root) {
        return Err(SensorError::Malformed);
    }
    read_attribute(path, 128).map_err(error)
}
impl SystemLinuxDataSource {
    fn gpu_path(&self, bdf: PciBdf) -> Result<PathBuf, SensorError> {
        let path = fs::canonicalize(self.rooted_path(&format!("/sys/bus/pci/devices/{bdf}")))
            .map_err(map_io_error)
            .map_err(error)?;
        let root = fs::canonicalize(self.rooted_path("/sys/devices"))
            .map_err(map_io_error)
            .map_err(error)?;
        if !path.starts_with(root)
            || path.file_name().and_then(|v| v.to_str()) != Some(bdf.to_string().as_str())
        {
            return Err(SensorError::Malformed);
        }
        Ok(path)
    }
}
impl GpuSensorSource for SystemLinuxDataSource {
    fn power(&self, bdf: PciBdf) -> RuntimePower {
        let value = self
            .gpu_path(bdf)
            .and_then(|p| contained_read(&p, &p.join("power/runtime_status")));
        match value.as_deref() {
            Ok(b"active\n") | Ok(b"active") => RuntimePower::Active,
            Ok(b"suspended\n") | Ok(b"suspended") => RuntimePower::Suspended,
            _ => RuntimePower::Unknown,
        }
    }
    fn attribute(&self, bdf: PciBdf, attribute: GpuAttribute) -> Result<Vec<u8>, SensorError> {
        let root = self.gpu_path(bdf)?;
        if attribute == GpuAttribute::IntelCurrentMhz {
            // i915's primary DRM node, verified by canonical containment.
            let entries = fs::read_dir(root.join("drm"))
                .map_err(map_io_error)
                .map_err(error)?;
            let mut values = Vec::new();
            for (i, entry) in entries.enumerate() {
                if i >= 128 {
                    return Err(SensorError::Malformed);
                }
                let entry = entry.map_err(map_io_error).map_err(error)?;
                if !crate::gpu_sources::is_primary_node_name(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                let path = fs::canonicalize(entry.path())
                    .map_err(map_io_error)
                    .map_err(error)?;
                if !path.starts_with(&root) {
                    return Err(SensorError::Malformed);
                }
                values.push(contained_read(&root, &path.join(attribute.name()))?);
            }
            return if values.len() == 1 {
                Ok(values.remove(0))
            } else if values.is_empty() {
                Err(SensorError::Missing)
            } else {
                Err(SensorError::Ambiguous)
            };
        }
        contained_read(&root, &root.join(attribute.name()))
    }
    fn hwmon(&self, bdf: PciBdf, driver: &str, metric: MetricId) -> Result<Vec<u8>, SensorError> {
        // Only documented AMD edge (temp1) and GPU power1 input semantics are
        // selected. Intel package/rail sensors are not total GPU sensors.
        if driver != "amdgpu" {
            return Err(SensorError::Unsupported);
        }
        let attribute = match metric {
            MetricId::GpuTemperature => "temp1_input",
            MetricId::GpuPower => "power1_input",
            _ => return Err(SensorError::Unsupported),
        };
        let root = self.gpu_path(bdf)?;
        let entries = fs::read_dir(root.join("hwmon"))
            .map_err(map_io_error)
            .map_err(error)?;
        let mut sensors = Vec::new();
        for (i, entry) in entries.enumerate() {
            if i >= 32 {
                return Err(SensorError::Malformed);
            }
            let entry = entry.map_err(map_io_error).map_err(error)?;
            let path = fs::canonicalize(entry.path())
                .map_err(map_io_error)
                .map_err(error)?;
            if !path.starts_with(&root) {
                return Err(SensorError::Malformed);
            }
            if read_attribute(&path.join("name"), 128)
                .map_err(error)?
                .as_slice()
                != b"amdgpu\n"
            {
                continue;
            }
            sensors.push(path);
        }
        if sensors.len() > 1 {
            return Err(SensorError::Ambiguous);
        }
        let path = sensors.first().ok_or(SensorError::Missing)?;
        contained_read(&root, &path.join(attribute))
    }
}
/// Strict integer parsing and checked normalization; no floating point source arithmetic.
pub fn unsigned(bytes: &[u8]) -> Result<u64, SensorError> {
    if bytes.len() > 128 {
        return Err(SensorError::Malformed);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| SensorError::Malformed)?
        .trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SensorError::Malformed);
    }
    text.parse().map_err(|_| SensorError::Malformed)
}
pub fn signed(bytes: &[u8]) -> Result<i64, SensorError> {
    if bytes.len() > 128 {
        return Err(SensorError::Malformed);
    }
    std::str::from_utf8(bytes)
        .map_err(|_| SensorError::Malformed)?
        .trim()
        .parse()
        .map_err(|_| SensorError::Malformed)
}
pub fn mhz_to_hz(value: u64) -> Result<u64, SensorError> {
    value.checked_mul(1_000_000).ok_or(SensorError::Malformed)
}
pub fn percent_to_basis_points(value: u64) -> Result<u64, SensorError> {
    if value > 100 {
        Err(SensorError::Malformed)
    } else {
        Ok(value * 100)
    }
}
/// Round down to the nearest whole milliwatt (less than 1 mW lost).
pub const fn microwatts_to_milliwatts(value: u64) -> u64 {
    value / 1000
}
