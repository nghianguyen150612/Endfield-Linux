//! Minimal audited NVML adapter. All unsafe code stays in this module.
//! Loader policy: system dynamic loader's `libnvidia-ml.so.1` search. No shell,
//! device index correlation, UUID retention, or MIG enumeration. Calls are
//! synchronous and cannot be forcibly cancelled inside the vendor library.
use crate::{
    PciBdf,
    gpu_telemetry_sources::{SensorError, mhz_to_hz},
};
use endfield_unix_domain::MeasurementValue;
use endfield_unix_telemetry::MetricId;
use std::ffi::{c_char, c_uint, c_void};
#[derive(Clone, Debug)]
pub struct NvidiaReading {
    pub bdf: PciBdf,
    pub values: Vec<(MetricId, Result<MeasurementValue, SensorError>)>,
}
pub trait NvidiaBackend: Send + Sync {
    fn collect(
        &self,
        devices: &[PciBdf],
        metrics: &[MetricId],
    ) -> Result<Vec<NvidiaReading>, SensorError>;
}
pub struct SystemNvml;
#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}
type Handle = *mut c_void;
type Return = c_uint;
fn status(code: Return) -> Result<(), SensorError> {
    match code {
        0 => Ok(()),
        3 => Err(SensorError::Unsupported),
        4 => Err(SensorError::Denied),
        _ => Err(SensorError::Temporary),
    }
}
struct Session {
    _library: libloading::Library,
    shutdown: unsafe extern "C" fn() -> Return,
}
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: symbol belongs to retained initialized library; exactly one shutdown per successful init.
        unsafe {
            (self.shutdown)();
        }
    }
}
impl NvidiaBackend for SystemNvml {
    fn collect(
        &self,
        devices: &[PciBdf],
        metrics: &[MetricId],
    ) -> Result<Vec<NvidiaReading>, SensorError> {
        if devices.is_empty() || metrics.is_empty() {
            return Ok(Vec::new());
        }
        if devices.len() > 128 || devices.iter().any(|b| !b.is_valid()) {
            return Err(SensorError::Malformed);
        }
        // SAFETY: only the documented system NVML SONAME is loaded. ABI types
        // below match nvml.h; outputs are initialized, correctly sized, and
        // never outlive the library. Handles come from physical BDF lookup.
        unsafe {
            let library =
                libloading::Library::new("libnvidia-ml.so.1").map_err(|_| SensorError::Missing)?;
            let init = *library
                .get::<unsafe extern "C" fn() -> Return>(b"nvmlInit_v2\0")
                .map_err(|_| SensorError::Unsupported)?;
            let shutdown = *library
                .get::<unsafe extern "C" fn() -> Return>(b"nvmlShutdown\0")
                .map_err(|_| SensorError::Unsupported)?;
            let by_pci = *library
                .get::<unsafe extern "C" fn(*const c_char, *mut Handle) -> Return>(
                    b"nvmlDeviceGetHandleByPciBusId_v2\0",
                )
                .map_err(|_| SensorError::Unsupported)?;
            let pci = *library
                .get::<unsafe extern "C" fn(Handle, *mut PciInfo) -> Return>(
                    b"nvmlDeviceGetPciInfo_v3\0",
                )
                .map_err(|_| SensorError::Unsupported)?;
            status(init())?;
            let session = Session {
                _library: library,
                shutdown,
            };
            let mut readings = Vec::new();
            for device in devices {
                let address = std::ffi::CString::new(device.to_string())
                    .map_err(|_| SensorError::Malformed)?;
                let mut handle = std::ptr::null_mut();
                if let Err(error) = status(by_pci(address.as_ptr(), &mut handle)) {
                    readings.push(NvidiaReading {
                        bdf: *device,
                        values: metrics.iter().map(|m| (*m, Err(error))).collect(),
                    });
                    continue;
                }
                if handle.is_null() {
                    return Err(SensorError::Malformed);
                }
                let mut info = PciInfo::default();
                status(pci(handle, &mut info))?;
                let bdf: PciBdf = format!(
                    "{:04x}:{:02x}:{:02x}.{}",
                    info.domain, info.bus, info.device, 0
                )
                .parse()
                .map_err(|_| SensorError::Malformed)?;
                // Validate full returned bus ID, including function, rather than
                // assuming function zero from the numeric fields.
                let end = info
                    .bus_id
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or(SensorError::Malformed)?;
                let raw: Vec<u8> = info.bus_id[..end].iter().map(|b| *b as u8).collect();
                let text = std::str::from_utf8(&raw).map_err(|_| SensorError::Malformed)?;
                // NVML uses an eight-hex-digit domain. P013 permits 16 bits.
                let text = if text.len() == 16 && text.starts_with("0000") {
                    &text[4..]
                } else {
                    text
                };
                let full: PciBdf = text.parse().map_err(|_| SensorError::Malformed)?;
                if full != *device {
                    return Err(SensorError::Ambiguous);
                }
                if full.to_string()[..11] != bdf.to_string()[..11] {
                    return Err(SensorError::Malformed);
                }
                let mut values = Vec::new();
                for &metric in metrics {
                    let value = match metric {
                        MetricId::GpuMemoryUsed | MetricId::GpuDedicatedMemoryCapacity => session
                            ._library
                            .get::<unsafe extern "C" fn(Handle, *mut Memory) -> Return>(
                                b"nvmlDeviceGetMemoryInfo\0",
                            )
                            .map_err(|_| SensorError::Unsupported)
                            .and_then(|f| {
                                let mut m = Memory::default();
                                status(f(handle, &mut m))?;
                                if m.used > m.total || m.free > m.total || m.total == 0 {
                                    return Err(SensorError::Malformed);
                                }
                                Ok(MeasurementValue::Unsigned(
                                    if metric == MetricId::GpuMemoryUsed {
                                        m.used
                                    } else {
                                        m.total
                                    },
                                ))
                            }),
                        MetricId::GpuTemperature => session
                            ._library
                            .get::<unsafe extern "C" fn(Handle, c_uint, *mut c_uint) -> Return>(
                                b"nvmlDeviceGetTemperature\0",
                            )
                            .map_err(|_| SensorError::Unsupported)
                            .and_then(|f| {
                                let mut v = 0;
                                status(f(handle, 0, &mut v))?;
                                Ok(MeasurementValue::Signed(i64::from(v) * 1000))
                            }),
                        MetricId::GpuPower => session
                            ._library
                            .get::<unsafe extern "C" fn(Handle, *mut c_uint) -> Return>(
                                b"nvmlDeviceGetPowerUsage\0",
                            )
                            .map_err(|_| SensorError::Unsupported)
                            .and_then(|f| {
                                let mut v = 0;
                                status(f(handle, &mut v))?;
                                Ok(MeasurementValue::Unsigned(u64::from(v)))
                            }),
                        MetricId::GpuFrequency => session
                            ._library
                            .get::<unsafe extern "C" fn(Handle, c_uint, *mut c_uint) -> Return>(
                                b"nvmlDeviceGetClockInfo\0",
                            )
                            .map_err(|_| SensorError::Unsupported)
                            .and_then(|f| {
                                let mut v = 0;
                                status(f(handle, 0, &mut v))?;
                                Ok(MeasurementValue::Unsigned(mhz_to_hz(u64::from(v))?))
                            }),
                        _ => Err(SensorError::Unsupported),
                    };
                    values.push((metric, value));
                }
                readings.push(NvidiaReading { bdf: full, values });
            }
            Ok(readings)
        }
    }
}
// nvmlPciInfo_t v3 ABI: legacy ID, numeric domain/bus/device/device IDs,
// followed by the 32-byte extended bus ID. No UUID or serial is requested.
#[repr(C)]
#[derive(Default)]
struct PciInfo {
    legacy: [c_char; 16],
    domain: c_uint,
    bus: c_uint,
    device: c_uint,
    pci_device_id: c_uint,
    pci_subsystem_id: c_uint,
    bus_id: [c_char; 32],
}
