use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::hardware_sources::{
    CgroupSource, CgroupV2Control, CgroupV2Files, CpuFrequencyField, CpuListSource,
    CpuTopologyField, LinuxHardwareDataSource, PlatformProfileField, RawSchedulerState,
};
use crate::kernel::RawKernelIdentity;
use crate::os_release::MAX_OS_RELEASE_BYTES;

const MAX_SYSTEMD_CONTAINER_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsReleaseFile {
    Etc,
    UsrLib,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerMarker {
    Docker,
    Podman,
    Systemd,
}

impl ContainerMarker {
    pub const fn source_id(self) -> &'static str {
        match self {
            Self::Docker => "linux-dockerenv",
            Self::Podman => "linux-containerenv",
            Self::Systemd => "linux-systemd-container",
        }
    }
}

/// Metadata needed to apply P010's runtime-directory policy without opening or
/// changing the directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeDirectoryMetadata {
    pub is_directory: bool,
    pub is_symlink: bool,
    pub owner_uid: u32,
    pub mode: u32,
}

/// Read-only, narrow source boundary for deterministic platform-probe tests.
/// Implementations cannot request arbitrary file reads through this trait.
pub trait LinuxDataSource: Send + Sync {
    fn read_os_release(&self, file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError>;
    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError>;
    fn container_marker_present(&self, marker: ContainerMarker) -> Result<bool, DataSourceError>;
    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError>;
    fn runtime_directory_metadata(
        &self,
        path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError>;
    fn effective_uid(&self) -> u32;

    /// Returns a truthful time of observation, if the system clock is usable.
    fn observation_time_unix_nanos(&self) -> Option<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }
}

/// Native implementation using fixed Linux metadata sources and safe `nix`
/// wrappers for `uname` and effective UID.
#[derive(Clone, Debug)]
pub struct SystemLinuxDataSource {
    root: PathBuf,
}

impl Default for SystemLinuxDataSource {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }
}

impl SystemLinuxDataSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses a virtual root while retaining the same fixed relative source list.
    /// This supports isolated filesystem-policy tests and does not enable
    /// general path-based file reads.
    #[doc(hidden)]
    pub fn rooted_at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, absolute: &str) -> PathBuf {
        self.root.join(absolute.trim_start_matches('/'))
    }

    pub(crate) fn rooted_path(&self, absolute: &str) -> PathBuf {
        self.path(absolute)
    }
}

impl LinuxDataSource for SystemLinuxDataSource {
    fn read_os_release(&self, file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        let etc = self.path("/etc/os-release");
        let usr_lib = self.path("/usr/lib/os-release");
        match file {
            OsReleaseFile::UsrLib => read_regular_bounded(&usr_lib, MAX_OS_RELEASE_BYTES),
            OsReleaseFile::Etc => match fs::symlink_metadata(&etc).map_err(map_io_error)? {
                metadata if metadata.file_type().is_symlink() => {
                    let target = fs::read_link(&etc).map_err(map_io_error)?;
                    if !is_allowed_os_release_target(&target) {
                        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
                    }
                    read_regular_bounded(&usr_lib, MAX_OS_RELEASE_BYTES)
                }
                metadata if metadata.is_file() => read_regular_bounded(&etc, MAX_OS_RELEASE_BYTES),
                _ => Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType)),
            },
        }
    }

    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        #[cfg(target_os = "linux")]
        {
            let uname = nix::sys::utsname::uname().map_err(|error| {
                let kind = if matches!(error, nix::errno::Errno::EACCES | nix::errno::Errno::EPERM)
                {
                    DataSourceErrorKind::PermissionDenied
                } else {
                    DataSourceErrorKind::Other
                };
                DataSourceError::new(kind)
            })?;
            let to_string = |value: &std::ffi::OsStr| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))
            };
            Ok(RawKernelIdentity {
                system_name: to_string(uname.sysname())?,
                release: to_string(uname.release())?,
                machine: to_string(uname.machine())?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(DataSourceError::new(DataSourceErrorKind::Unsupported))
        }
    }

    fn container_marker_present(&self, marker: ContainerMarker) -> Result<bool, DataSourceError> {
        let path = match marker {
            ContainerMarker::Docker => self.path("/.dockerenv"),
            ContainerMarker::Podman => self.path("/run/.containerenv"),
            ContainerMarker::Systemd => {
                let path = self.path("/run/systemd/container");
                let bytes = match read_regular_bounded(&path, MAX_SYSTEMD_CONTAINER_BYTES) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error),
                };
                let value = std::str::from_utf8(&bytes)
                    .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))?
                    .trim()
                    .to_ascii_lowercase();
                if value.is_empty() {
                    return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
                }
                return Ok(matches!(
                    value.as_str(),
                    "docker"
                        | "podman"
                        | "lxc"
                        | "lxc-libvirt"
                        | "systemd-nspawn"
                        | "nspawn"
                        | "openvz"
                        | "rkt"
                ));
            }
        };

        match validate_regular_marker(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind == DataSourceErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        Ok(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
    }

    fn runtime_directory_metadata(
        &self,
        path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
        Ok(RuntimeDirectoryMetadata {
            is_directory: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
            owner_uid: metadata.uid(),
            mode: metadata.permissions().mode(),
        })
    }

    fn effective_uid(&self) -> u32 {
        #[cfg(unix)]
        {
            nix::unistd::Uid::effective().as_raw()
        }
        #[cfg(not(unix))]
        {
            0
        }
    }
}

impl LinuxHardwareDataSource for SystemLinuxDataSource {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        read_regular_bounded(&self.path("/proc/cpuinfo"), 256 * 1024)
    }

    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        read_regular_bounded(&self.path("/proc/meminfo"), 64 * 1024)
    }

    fn read_proc_stat(&self) -> Result<Vec<u8>, DataSourceError> {
        read_regular_bounded(
            &self.path("/proc/stat"),
            crate::cpu_stat::MAX_PROC_STAT_BYTES,
        )
    }

    fn read_cpu_list(&self, source: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        let name = match source {
            CpuListSource::Online => "online",
            CpuListSource::Present => "present",
            CpuListSource::Possible => "possible",
        };
        read_regular_bounded(
            &self.path(&format!("/sys/devices/system/cpu/{name}")),
            16 * 1024,
        )
    }

    fn read_cpu_topology_field(
        &self,
        cpu: u32,
        field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        let filename = match field {
            CpuTopologyField::PhysicalPackageId => "physical_package_id",
            CpuTopologyField::CoreId => "core_id",
            CpuTopologyField::DieId => "die_id",
        };
        let system_cpu = self.path("/sys/devices/system/cpu");
        let directory = system_cpu.join(format!("cpu{cpu}/topology"));
        let directory = match canonical_directory_in(&directory, &system_cpu) {
            Ok(directory) => directory,
            Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let bytes = match read_regular_bounded(&directory.join(filename), 64) {
            Ok(bytes) => bytes,
            Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))?
            .trim();
        if text.is_empty() || text.len() > 32 {
            return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
        }
        let value = text
            .parse::<i64>()
            .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
        Ok(Some(value))
    }

    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        system_process_affinity()
    }

    fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
        system_scheduler_state()
    }

    fn enumerate_cpufreq_policies(&self) -> Result<Vec<u32>, DataSourceError> {
        const MAX_POLICIES: usize = 256;
        let system_cpu = self.path("/sys/devices/system/cpu");
        let policy_root = system_cpu.join("cpufreq");
        let policy_root = canonical_directory_in(&policy_root, &system_cpu)?;
        let entries = fs::read_dir(&policy_root).map_err(map_io_error)?;
        let mut policies = Vec::new();
        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(index) = name.strip_prefix("policy") else {
                continue;
            };
            if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            let index = index
                .parse::<u32>()
                .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
            let _ = canonical_directory_in(&entry.path(), &system_cpu)?;
            if policies.len() == MAX_POLICIES {
                return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
            }
            policies.push(index);
        }
        policies.sort_unstable();
        policies.dedup();
        Ok(policies)
    }

    fn read_cpufreq_policy_field(
        &self,
        policy: u32,
        field: CpuFrequencyField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        let filename = match field {
            CpuFrequencyField::ScalingDriver => "scaling_driver",
            CpuFrequencyField::ScalingGovernor => "scaling_governor",
            CpuFrequencyField::AvailableGovernors => "scaling_available_governors",
            CpuFrequencyField::CpuInfoMinFrequencyKhz => "cpuinfo_min_freq",
            CpuFrequencyField::CpuInfoMaxFrequencyKhz => "cpuinfo_max_freq",
            CpuFrequencyField::CpuInfoCurrentFrequencyKhz => "cpuinfo_cur_freq",
            CpuFrequencyField::ScalingMinFrequencyKhz => "scaling_min_freq",
            CpuFrequencyField::ScalingMaxFrequencyKhz => "scaling_max_freq",
            CpuFrequencyField::RelatedCpus => "related_cpus",
            CpuFrequencyField::AffectedCpus => "affected_cpus",
            CpuFrequencyField::EnergyPerformancePreference => "energy_performance_preference",
            CpuFrequencyField::AvailableEnergyPerformancePreferences => {
                "energy_performance_available_preferences"
            }
        };
        let system_cpu = self.path("/sys/devices/system/cpu");
        let policy_dir = system_cpu.join(format!("cpufreq/policy{policy}"));
        let policy_dir = match canonical_directory_in(&policy_dir, &system_cpu) {
            Ok(directory) => directory,
            Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match read_regular_bounded(&policy_dir.join(filename), 4096) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind == DataSourceErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn read_platform_profile_field(
        &self,
        field: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        let filename = match field {
            PlatformProfileField::Current => "platform_profile",
            PlatformProfileField::Available => "platform_profile_choices",
        };
        let acpi = self.path("/sys/firmware/acpi");
        let directory = match canonical_directory_in(&acpi, &self.path("/sys/firmware")) {
            Ok(directory) => directory,
            Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match read_regular_bounded(&directory.join(filename), 4096) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind == DataSourceErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        let cgroups = read_regular_bounded(&self.path("/proc/self/cgroup"), 32 * 1024)?;
        let mountinfo = read_regular_bounded(&self.path("/proc/self/mountinfo"), 256 * 1024)?;
        let cgroup_path = parse_unified_cgroup_path(&cgroups)?;
        let mounts = parse_cgroup_mounts(&mountinfo)?;
        let Some(cgroup_path) = cgroup_path else {
            if mounts.iter().any(|mount| mount.fs_type == "cgroup") {
                return Ok(CgroupSource::V1);
            }
            if mounts.iter().any(|mount| mount.fs_type == "cgroup2") {
                return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
            }
            return Ok(CgroupSource::None);
        };
        let Some((_, mount, candidate)) = mounts
            .iter()
            .filter(|mount| mount.fs_type == "cgroup2")
            .filter_map(|mount| {
                map_cgroup_path(mount, &cgroup_path, &self.root)
                    .map(|(depth, path)| (depth, mount, path))
            })
            .max_by_key(|(root_depth, _, _)| *root_depth)
        else {
            if mounts.iter().any(|mount| mount.fs_type == "cgroup2") {
                return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
            }
            return Err(DataSourceError::new(DataSourceErrorKind::NotFound));
        };
        let mount_path = self.rooted_path(
            mount
                .mountpoint
                .to_str()
                .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::MalformedData))?,
        );
        let canonical_mount = fs::canonicalize(&mount_path).map_err(map_io_error)?;
        let canonical_root = fs::canonicalize(&self.root).map_err(map_io_error)?;
        let canonical_cgroup = fs::canonicalize(&candidate).map_err(map_io_error)?;
        if !canonical_mount.starts_with(&canonical_root)
            || !canonical_cgroup.starts_with(&canonical_mount)
        {
            return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
        }
        let read_control = |control| read_cgroup_control(&canonical_cgroup, control);
        Ok(CgroupSource::V2(CgroupV2Files {
            cpu_max: read_control(CgroupV2Control::CpuMax),
            cpuset_cpus_effective: read_control(CgroupV2Control::CpusetCpusEffective),
            memory_max: read_control(CgroupV2Control::MemoryMax),
            memory_current: read_control(CgroupV2Control::MemoryCurrent),
            memory_swap_max: read_control(CgroupV2Control::MemorySwapMax),
        }))
    }
}

#[derive(Clone, Debug)]
struct CgroupMount {
    root: PathBuf,
    mountpoint: PathBuf,
    fs_type: String,
}

fn canonical_directory_in(path: &Path, boundary: &Path) -> Result<PathBuf, DataSourceError> {
    let canonical_boundary = fs::canonicalize(boundary).map_err(map_io_error)?;
    let canonical_path = fs::canonicalize(path).map_err(map_io_error)?;
    let metadata = fs::metadata(&canonical_path).map_err(map_io_error)?;
    if !metadata.is_dir() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    if !canonical_path.starts_with(&canonical_boundary) {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    Ok(canonical_path)
}

fn parse_unified_cgroup_path(bytes: &[u8]) -> Result<Option<PathBuf>, DataSourceError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))?;
    if text.trim().is_empty() {
        return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
    }
    let mut unified = None;
    for line in text.lines() {
        if line.len() > 4096 {
            return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
        }
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next().unwrap_or_default();
        let controllers = fields.next().unwrap_or_default();
        let path = fields
            .next()
            .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
        if hierarchy == "0" && controllers.is_empty() {
            if unified.is_some() {
                return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
            }
            unified = Some(validated_absolute_path(path)?);
        }
    }
    Ok(unified)
}

fn parse_cgroup_mounts(bytes: &[u8]) -> Result<Vec<CgroupMount>, DataSourceError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))?;
    let mut mounts = Vec::new();
    for line in text.lines() {
        if line.len() > 8192 {
            return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
        }
        let (left, right) = line
            .split_once(" - ")
            .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
        let left_fields = left.split_ascii_whitespace().collect::<Vec<_>>();
        let right_fields = right.split_ascii_whitespace().collect::<Vec<_>>();
        if left_fields.len() < 6 || right_fields.len() < 3 {
            return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
        }
        let fs_type = right_fields[0];
        if fs_type != "cgroup2" && fs_type != "cgroup" {
            continue;
        }
        let root = decode_mountinfo_path(left_fields[3])?;
        let mountpoint = decode_mountinfo_path(left_fields[4])?;
        mounts.push(CgroupMount {
            root: validated_absolute_path(&root)?,
            mountpoint: validated_absolute_path(&mountpoint)?,
            fs_type: fs_type.to_owned(),
        });
    }
    Ok(mounts)
}

fn decode_mountinfo_path(encoded: &str) -> Result<String, DataSourceError> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            let escape = bytes
                .get(cursor + 1..cursor + 4)
                .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
            let value = match escape {
                b"040" => b' ',
                b"011" => b'\t',
                b"012" => b'\n',
                b"134" => b'\\',
                _ => return Err(DataSourceError::new(DataSourceErrorKind::MalformedData)),
            };
            decoded.push(value);
            cursor += 4;
        } else {
            decoded.push(bytes[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))
}

fn validated_absolute_path(value: &str) -> Result<PathBuf, DataSourceError> {
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    Ok(path.to_path_buf())
}

fn map_cgroup_path(
    mount: &CgroupMount,
    cgroup_path: &Path,
    root: &Path,
) -> Option<(usize, PathBuf)> {
    let relative = cgroup_path.strip_prefix(&mount.root).ok()?;
    let rooted_mount = root.join(mount.mountpoint.strip_prefix("/").ok()?);
    Some((mount.root.components().count(), rooted_mount.join(relative)))
}

fn read_cgroup_control(
    cgroup_dir: &Path,
    control: CgroupV2Control,
) -> Result<Option<Vec<u8>>, DataSourceError> {
    let filename = match control {
        CgroupV2Control::CpuMax => "cpu.max",
        CgroupV2Control::CpusetCpusEffective => "cpuset.cpus.effective",
        CgroupV2Control::MemoryMax => "memory.max",
        CgroupV2Control::MemoryCurrent => "memory.current",
        CgroupV2Control::MemorySwapMax => "memory.swap.max",
    };
    match read_regular_bounded(&cgroup_dir.join(filename), 4096) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind == DataSourceErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn system_process_affinity() -> Result<Vec<u32>, DataSourceError> {
    const MAX_MASK_BYTES: usize = 1024 * 1024;
    let mut mask_bytes = 128usize;
    loop {
        let words = mask_bytes.div_ceil(std::mem::size_of::<usize>());
        let mut mask = vec![0usize; words];
        let allocated_bytes = words * std::mem::size_of::<usize>();
        // SAFETY: `mask` is word-aligned, initialized, and writable for the
        // exact size passed to sched_getaffinity. The pid 0 query is read-only.
        let result = unsafe {
            libc::sched_getaffinity(
                0,
                allocated_bytes,
                mask.as_mut_ptr().cast::<libc::cpu_set_t>(),
            )
        };
        if result == 0 {
            let mut cpus = Vec::new();
            for (word_index, word) in mask.into_iter().enumerate() {
                for bit in 0..usize::BITS {
                    if word & (1usize << bit) != 0 {
                        let cpu = word_index
                            .checked_mul(usize::BITS as usize)
                            .and_then(|base| base.checked_add(bit as usize))
                            .and_then(|cpu| u32::try_from(cpu).ok())
                            .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::TooLarge))?;
                        cpus.push(cpu);
                    }
                }
            }
            if cpus.is_empty() {
                return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
            }
            return Ok(cpus);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINVAL) && mask_bytes < MAX_MASK_BYTES {
            mask_bytes = (mask_bytes * 2).min(MAX_MASK_BYTES);
            continue;
        }
        let kind = match error.kind() {
            io::ErrorKind::PermissionDenied => DataSourceErrorKind::PermissionDenied,
            _ if error.raw_os_error() == Some(libc::ENOSYS) => DataSourceErrorKind::Unsupported,
            _ if error.raw_os_error() == Some(libc::EINVAL) => DataSourceErrorKind::TooLarge,
            _ => DataSourceErrorKind::Other,
        };
        return Err(DataSourceError::new(kind));
    }
}

fn system_scheduler_state() -> Result<RawSchedulerState, DataSourceError> {
    // SAFETY: a pid of zero asks the kernel for the current process policy.
    let policy = unsafe { libc::sched_getscheduler(0) };
    if policy < 0 {
        return Err(map_scheduler_errno());
    }
    // SAFETY: sched_param is a C output structure fully initialized by the
    // kernel on a successful read-only query.
    let mut parameters = unsafe { std::mem::zeroed::<libc::sched_param>() };
    // SAFETY: `parameters` is a valid, writable sched_param for pid zero.
    if unsafe { libc::sched_getparam(0, &mut parameters) } != 0 {
        return Err(map_scheduler_errno());
    }
    Ok(RawSchedulerState {
        policy,
        priority: parameters.sched_priority,
    })
}

fn map_scheduler_errno() -> DataSourceError {
    let error = io::Error::last_os_error();
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => DataSourceErrorKind::PermissionDenied,
        _ if error.raw_os_error() == Some(libc::ENOSYS) => DataSourceErrorKind::Unsupported,
        _ => DataSourceErrorKind::Other,
    };
    DataSourceError::new(kind)
}

fn read_regular_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, DataSourceError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    if metadata.size() > limit as u64 {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(map_io_error)?;
    read_open_file_bounded(file, limit)
}

fn validate_regular_marker(path: &Path) -> Result<(), DataSourceError> {
    use std::os::unix::fs::OpenOptionsExt;

    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(map_io_error)?;
    if !file.metadata().map_err(map_io_error)?.is_file() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    Ok(())
}

fn read_open_file_bounded(file: File, limit: usize) -> Result<Vec<u8>, DataSourceError> {
    if !file.metadata().map_err(map_io_error)?.file_type().is_file() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(map_io_error)?;
    if bytes.len() > limit {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    Ok(bytes)
}

fn is_allowed_os_release_target(target: &Path) -> bool {
    if target == Path::new("/usr/lib/os-release") {
        return true;
    }
    if target.is_absolute() {
        return false;
    }
    let mut resolved = PathBuf::from("/");
    resolved.push("etc");
    for component in target.components() {
        match component {
            Component::Normal(value) => resolved.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved == Path::new("/") {
                    return false;
                }
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved == Path::new("/usr/lib/os-release")
}

fn map_io_error(error: io::Error) -> DataSourceError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => DataSourceErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => DataSourceErrorKind::PermissionDenied,
        io::ErrorKind::InvalidData => DataSourceErrorKind::InvalidUtf8,
        _ => DataSourceErrorKind::Other,
    };
    DataSourceError::new(kind)
}
