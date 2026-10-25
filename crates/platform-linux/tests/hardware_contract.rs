use std::collections::HashMap;
use std::fs;
use std::path::Path;

use endfield_unix_domain::{Availability, CapabilityId, MeasurementUnit};
use endfield_unix_platform_linux::{
    CgroupLimit, CgroupSource, CgroupStatus, CgroupV2Files, CpuFrequencyField, CpuListSource,
    CpuTimeQuota, CpuTopologyField, DataSourceError, DataSourceErrorKind, LinuxDataSource,
    LinuxHardwareDataSource, LinuxHardwareProbe, MemInfoFieldError, OsReleaseFile,
    PlatformProfileField, RawKernelIdentity, RawSchedulerState, RuntimeDirectoryMetadata,
    SystemLinuxDataSource, parse_cpu_list, parse_cpu_model, parse_meminfo,
};
use tempfile::tempdir;

#[test]
fn cpu_list_parser_handles_single_ranges_noncontiguous_ids_and_rejects_invalid_data() {
    assert_eq!(parse_cpu_list(b"0\n"), Ok(vec![0]));
    assert_eq!(
        parse_cpu_list(b"0-3,8,10-11\n"),
        Ok(vec![0, 1, 2, 3, 8, 10, 11])
    );
    assert_eq!(
        parse_cpu_list(b"0,0"),
        Err(endfield_unix_platform_linux::CpuListParseError::DuplicateCpu)
    );
    assert_eq!(
        parse_cpu_list(b"3-1"),
        Err(endfield_unix_platform_linux::CpuListParseError::InvalidRange)
    );
    assert!(parse_cpu_list(b"0-1000001").is_err());
    assert!(parse_cpu_list(&vec![b'0'; 16 * 1024 + 1]).is_err());
    assert!(parse_cpu_list(&[0xff]).is_err());
}

#[test]
fn cpu_model_parser_accepts_architecture_fields_and_ignores_unrelated_identifiers() {
    assert_eq!(
        parse_cpu_model(b"processor : 0\nmodel name : Example CPU 9000\nserial : private\n"),
        Ok(Some("Example CPU 9000".to_owned()))
    );
    assert_eq!(
        parse_cpu_model(b"Processor : ARMv8 Processor rev 4 (v8l)\n"),
        Ok(Some("ARMv8 Processor rev 4 (v8l)".to_owned()))
    );
    assert_eq!(parse_cpu_model(b"processor : 0\n"), Ok(None));
    assert!(parse_cpu_model(&[0xff]).is_err());
}

#[test]
fn cpu_topology_distinguishes_online_present_possible_and_process_affinity() {
    let report = LinuxHardwareProbe::with_source(FixtureSource::standard())
        .inspect()
        .expect("fixture hardware probe");
    let cpu = report.cpu_topology.expect("CPU topology facts");
    assert_eq!(cpu.online_cpu_ids, vec![0, 1, 2, 3]);
    assert_eq!(cpu.present_cpu_ids, Some(vec![0, 1, 2, 3, 4, 5]));
    assert_eq!(cpu.possible_cpu_ids, Some((0..=7).collect()));
    assert_eq!(cpu.offline_cpu_ids, Some(vec![4, 5]));
    assert_eq!(cpu.process_allowed_cpu_ids, Some(vec![1, 3]));
    assert_eq!(cpu.physical_core_count, Some(2));
    assert_eq!(
        report
            .snapshot
            .cpu
            .as_ref()
            .unwrap()
            .topology
            .value
            .as_ref()
            .unwrap()
            .logical_processor_count,
        4
    );
}

#[test]
fn a_single_online_cpu_is_one_logical_processor_and_one_verified_core() {
    let mut source = FixtureSource::standard();
    source.set_cpu_sets(b"2\n", b"2\n", b"2\n");
    source.set_topology(2, 0, None, 7);
    source.affinity = Ok(vec![2]);
    let report = LinuxHardwareProbe::with_source(source)
        .inspect()
        .expect("single CPU probe");
    let topology = report.snapshot.cpu.unwrap().topology.value.unwrap();
    assert_eq!(topology.logical_processor_count, 1);
    assert_eq!(topology.physical_core_count, Some(1));
    assert_eq!(report.scheduling.allowed_cpu_ids, Some(vec![2]));
}

#[test]
fn physical_core_identity_includes_package_and_die_and_does_not_guess_hybrid_labels() {
    let mut source = FixtureSource::standard();
    source.set_cpu_sets(b"0,4\n", b"0,4\n", b"0,4\n");
    source.set_topology(0, 0, Some(0), 0);
    source.set_topology(4, 1, Some(0), 0);
    let report = LinuxHardwareProbe::with_source(source)
        .inspect()
        .expect("multi-package probe");
    let cpu = report.cpu_topology.as_ref().unwrap();
    assert_eq!(cpu.physical_core_count, Some(2));
    assert_eq!(cpu.online_cpu_ids, vec![0, 4]);
    assert!(!format!("{report:?}").contains("P-core"));
    assert!(!format!("{report:?}").contains("E-core"));
}

#[test]
fn missing_topology_metadata_keeps_logical_count_and_leaves_cores_unknown() {
    let mut source = FixtureSource::standard();
    source.topology.clear();
    let report = LinuxHardwareProbe::with_source(source)
        .inspect()
        .expect("partial topology probe");
    let topology = report.snapshot.cpu.unwrap().topology;
    assert_eq!(topology.availability, Availability::Available);
    assert_eq!(topology.value.unwrap().logical_processor_count, 4);
    assert_eq!(report.cpu_topology.unwrap().physical_core_count, None);
    assert_eq!(
        report.completeness,
        endfield_unix_platform_linux::LinuxHardwareCompleteness::Partial
    );
}

#[test]
fn affinity_supports_noncontiguous_and_large_cpu_indexes() {
    let mut source = FixtureSource::standard();
    source.affinity = Ok(vec![2, 2048, 4096]);
    let report = LinuxHardwareProbe::with_source(source)
        .inspect()
        .expect("large affinity probe");
    assert_eq!(report.scheduling.allowed_cpu_ids, Some(vec![2, 2048, 4096]));
    assert_eq!(
        report.cpu_topology.unwrap().process_allowed_cpu_ids,
        Some(vec![2, 2048, 4096])
    );
    assert_eq!(
        report.scheduling.mutation_permission,
        endfield_unix_platform_linux::QueryAvailability::Unknown
    );
}

#[test]
fn scheduler_query_permission_failure_does_not_claim_policy_change_permission() {
    let mut source = FixtureSource::standard();
    source.scheduler = Err(DataSourceError::new(DataSourceErrorKind::PermissionDenied));
    let report = LinuxHardwareProbe::with_source(source)
        .inspect()
        .expect("partial scheduler probe");
    assert_eq!(
        report.scheduling.scheduler_policy_query,
        endfield_unix_platform_linux::QueryAvailability::PermissionRestricted
    );
    assert_eq!(report.scheduling.scheduler_policy, None);
    assert_eq!(
        report.scheduling.mutation_permission,
        endfield_unix_platform_linux::QueryAvailability::Unknown
    );
}

#[test]
fn meminfo_parser_converts_kib_and_preserves_missing_and_malformed_fields() {
    let parsed = parse_meminfo(
        b"SwapFree: 32 kB\nMemAvailable: 512 kB\nSwapTotal: 1024 kB\nMemTotal: 2048 kB\n",
    )
    .expect("meminfo");
    assert_eq!(parsed.mem_total_bytes, Some(Ok(2 * 1024 * 1024)));
    assert_eq!(parsed.mem_available_bytes, Some(Ok(512 * 1024)));
    assert_eq!(parsed.swap_total_bytes, Some(Ok(1024 * 1024)));
    assert_eq!(parsed.swap_free_bytes, Some(Ok(32 * 1024)));

    let missing = parse_meminfo(b"MemTotal: 1 kB\n").expect("missing optional fields");
    assert_eq!(missing.mem_available_bytes, None);
    assert_eq!(missing.swap_total_bytes, None);

    let duplicate = parse_meminfo(b"MemTotal: 1 kB\nMemTotal: 2 kB\n").expect("duplicate field");
    assert_eq!(
        duplicate.mem_total_bytes,
        Some(Err(MemInfoFieldError::DuplicateField))
    );
    let invalid_unit = parse_meminfo(b"MemTotal: 1 MB\n").unwrap();
    assert_eq!(
        invalid_unit.mem_total_bytes,
        Some(Err(MemInfoFieldError::InvalidUnit))
    );
    let negative = parse_meminfo(b"MemTotal: -1 kB\n").unwrap();
    assert_eq!(
        negative.mem_total_bytes,
        Some(Err(MemInfoFieldError::InvalidInteger))
    );
    let overflow = parse_meminfo(b"MemTotal: 18446744073709551615 kB\n").unwrap();
    assert_eq!(
        overflow.mem_total_bytes,
        Some(Err(MemInfoFieldError::Overflow))
    );
    assert!(parse_meminfo(&[0xff]).is_err());
    assert!(parse_meminfo(&vec![b'X'; 64 * 1024 + 1]).is_err());
}

#[test]
fn swap_disabled_is_distinct_from_missing_swap_data() {
    let mut disabled = FixtureSource::standard();
    disabled.meminfo =
        Ok(b"MemTotal: 1024 kB\nMemAvailable: 512 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n".to_vec());
    let disabled = LinuxHardwareProbe::with_source(disabled).inspect().unwrap();
    let swap = disabled.snapshot.memory.unwrap().swap;
    assert_eq!(swap.availability, Availability::Available);
    assert_eq!(swap.present, Some(false));
    assert_eq!(swap.total, None);

    let mut missing = FixtureSource::standard();
    missing.meminfo = Ok(b"MemTotal: 1024 kB\n".to_vec());
    let missing = LinuxHardwareProbe::with_source(missing).inspect().unwrap();
    let swap = missing.snapshot.memory.unwrap().swap;
    assert_eq!(swap.availability, Availability::Unknown);
    assert_eq!(swap.present, None);
    assert_eq!(swap.total, None);
}

#[test]
fn invalid_available_memory_degrades_ram_without_losing_total_capacity() {
    let mut source = FixtureSource::standard();
    source.meminfo = Ok(b"MemTotal: 1024 kB\nMemAvailable: 2048 kB\nSwapTotal: 0 kB\n".to_vec());
    let report = LinuxHardwareProbe::with_source(source).inspect().unwrap();
    let memory = report.snapshot.memory.unwrap();
    assert_eq!(memory.ram.availability, Availability::Degraded);
    let capacity = memory.ram.value.as_ref().unwrap();
    assert_eq!(capacity.total.unwrap().0, 1024 * 1024);
    assert_eq!(capacity.available, None);
    let ram_capability = report
        .snapshot
        .capabilities
        .iter()
        .find(|capability| capability.id == CapabilityId::Ram)
        .unwrap();
    assert_eq!(
        ram_capability.measurement.unwrap().unit(),
        MeasurementUnit::Bytes
    );
}

#[test]
fn cgroup_v2_parsers_keep_fractional_quota_and_limits_separate_from_host_values() {
    let mut source = FixtureSource::standard();
    source.cgroup = Ok(CgroupSource::V2(cgroup_files(
        b"150000 100000\n",
        Some(b"2,4-5\n"),
        b"4294967296\n",
        b"1234\n",
        b"0\n",
    )));
    source.meminfo = Ok(b"MemTotal: 32768 kB\nMemAvailable: 16384 kB\nSwapTotal: 0 kB\n".to_vec());
    let report = LinuxHardwareProbe::with_source(source).inspect().unwrap();
    assert_eq!(
        report.cgroup.cpu_quota,
        CgroupLimit::Limited(CpuTimeQuota {
            quota_microseconds: 150000,
            period_microseconds: 100000
        })
    );
    assert_eq!(report.cgroup.effective_cpu_set, Some(vec![2, 4, 5]));
    assert_eq!(
        report.cgroup.memory_limit_bytes,
        CgroupLimit::Limited(4294967296)
    );
    assert_eq!(report.cgroup.memory_current_bytes, Some(1234));
    assert_eq!(
        report.cgroup.memory_swap_limit_bytes,
        CgroupLimit::Limited(0)
    );
    assert_eq!(report.memory.host_total_bytes, Some(32768 * 1024));
    assert_ne!(
        report.memory.host_total_bytes,
        match report.cgroup.memory_limit_bytes {
            CgroupLimit::Limited(value) => Some(value),
            _ => None,
        }
    );
}

#[test]
fn cgroup_unlimited_and_invalid_quota_period_are_distinct() {
    let unlimited = FixtureSource::standard();
    let report = LinuxHardwareProbe::with_source(unlimited)
        .inspect()
        .unwrap();
    assert_eq!(report.cgroup.cpu_quota, CgroupLimit::Unlimited);
    assert_eq!(report.cgroup.memory_limit_bytes, CgroupLimit::Unlimited);

    let mut invalid = FixtureSource::standard();
    invalid.cgroup = Ok(CgroupSource::V2(cgroup_files(
        b"1000 0\n",
        Some(b"\n"),
        b"max\n",
        b"0\n",
        b"max\n",
    )));
    let report = LinuxHardwareProbe::with_source(invalid).inspect().unwrap();
    assert_eq!(report.cgroup.cpu_quota, CgroupLimit::Unknown);
    assert_eq!(report.cgroup.effective_cpu_set, None);
    assert!(
        report
            .snapshot
            .diagnostics
            .iter()
            .any(|item| item.provider.as_deref() == Some("linux-cgroup"))
    );
}

#[test]
fn cgroup_v1_is_reported_as_partial_unsupported_behavior() {
    let mut source = FixtureSource::standard();
    source.cgroup = Ok(CgroupSource::V1);
    let report = LinuxHardwareProbe::with_source(source).inspect().unwrap();
    assert_eq!(report.cgroup.status, CgroupStatus::V1Unsupported);
    assert_eq!(report.cgroup.memory_limit_bytes, CgroupLimit::Unknown);
    assert_eq!(
        report.completeness,
        endfield_unix_platform_linux::LinuxHardwareCompleteness::Partial
    );
}

#[test]
fn cgroup_mount_resolution_handles_nested_paths_and_rejects_traversal() {
    let root = tempdir().unwrap();
    write(root.path(), "proc/self/cgroup", b"0::/tenant/team/job\n");
    write(
        root.path(),
        "proc/self/mountinfo",
        b"27 25 0:30 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw\n",
    );
    let group = root.path().join("sys/fs/cgroup/tenant/team/job");
    fs::create_dir_all(&group).unwrap();
    fs::write(group.join("cpu.max"), "max 100000\n").unwrap();
    fs::write(group.join("cpuset.cpus.effective"), "0-1\n").unwrap();
    fs::write(group.join("memory.max"), "max\n").unwrap();
    fs::write(group.join("memory.current"), "3\n").unwrap();
    fs::write(group.join("memory.swap.max"), "0\n").unwrap();
    let source = SystemLinuxDataSource::rooted_at(root.path());
    assert!(matches!(
        LinuxHardwareDataSource::cgroup_source(&source).unwrap(),
        CgroupSource::V2(_)
    ));

    write(root.path(), "proc/self/cgroup", b"0::/../../etc\n");
    assert_eq!(
        LinuxHardwareDataSource::cgroup_source(&source)
            .unwrap_err()
            .kind,
        DataSourceErrorKind::UnsafeSource
    );

    write(root.path(), "proc/self/cgroup", b"0::/tenant/team/job\n");
    write(
        root.path(),
        "proc/self/mountinfo",
        b"28 25 0:31 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup cgroup rw,cpu\n",
    );
    assert_eq!(
        LinuxHardwareDataSource::cgroup_source(&source)
            .unwrap_err()
            .kind,
        DataSourceErrorKind::NotFound
    );
}

#[test]
fn cgroup_v2_mount_without_a_resolvable_process_membership_is_unknown() {
    let root = tempdir().unwrap();
    write(root.path(), "proc/self/cgroup", b"2:cpu:/legacy\n");
    write(
        root.path(),
        "proc/self/mountinfo",
        b"27 25 0:30 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw\n",
    );
    let source = SystemLinuxDataSource::rooted_at(root.path());
    let report = LinuxHardwareProbe::with_source(source.clone())
        .inspect()
        .expect("partial hardware probe");
    assert_eq!(report.cgroup.status, CgroupStatus::Unknown);

    write(root.path(), "proc/self/cgroup", b"0::/tenant/team/job\n");
    write(
        root.path(),
        "proc/self/mountinfo",
        b"27 25 0:30 /different /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw\n",
    );
    assert_eq!(
        LinuxHardwareDataSource::cgroup_source(&source)
            .unwrap_err()
            .kind,
        DataSourceErrorKind::UnsafeSource
    );
}

#[test]
fn cpufreq_policy_symlinks_are_allowed_only_within_the_fixed_cpu_root() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let cpu_root = root.path().join("sys/devices/system/cpu");
    let cpufreq = cpu_root.join("cpufreq");
    fs::create_dir_all(&cpufreq).unwrap();
    let inside = cpufreq.join("policy0-target");
    fs::create_dir_all(&inside).unwrap();
    symlink(&inside, cpufreq.join("policy0")).unwrap();
    let source = SystemLinuxDataSource::rooted_at(root.path());
    assert_eq!(
        LinuxHardwareDataSource::enumerate_cpufreq_policies(&source).unwrap(),
        vec![0]
    );

    let outside = root.path().join("sys/devices/other-policy");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, cpufreq.join("policy1")).unwrap();
    assert_eq!(
        LinuxHardwareDataSource::enumerate_cpufreq_policies(&source)
            .unwrap_err()
            .kind,
        DataSourceErrorKind::UnsafeSource
    );
}

#[test]
fn cpufreq_reports_policy_limits_in_hz_without_claiming_write_permission() {
    let mut source = FixtureSource::standard();
    source.policies = Ok(vec![0]);
    source.set_frequency_policy(0, CpuFrequencyField::ScalingDriver, b"intel_pstate");
    source.set_frequency_policy(0, CpuFrequencyField::ScalingGovernor, b"powersave");
    source.set_frequency_policy(
        0,
        CpuFrequencyField::AvailableGovernors,
        b"performance powersave schedutil",
    );
    source.set_frequency_policy(0, CpuFrequencyField::CpuInfoMinFrequencyKhz, b"800000");
    source.set_frequency_policy(0, CpuFrequencyField::CpuInfoMaxFrequencyKhz, b"4800000");
    source.set_frequency_policy(0, CpuFrequencyField::ScalingMinFrequencyKhz, b"1200000");
    source.set_frequency_policy(0, CpuFrequencyField::ScalingMaxFrequencyKhz, b"4000000");
    source.set_frequency_policy(0, CpuFrequencyField::RelatedCpus, b"0-3");
    source.set_frequency_policy(0, CpuFrequencyField::AffectedCpus, b"0-3");
    source.set_frequency_policy(
        0,
        CpuFrequencyField::EnergyPerformancePreference,
        b"balance_performance",
    );
    source.set_frequency_policy(
        0,
        CpuFrequencyField::AvailableEnergyPerformancePreferences,
        b"performance balance_performance power",
    );
    source
        .profile
        .insert(PlatformProfileField::Current, Some(b"balanced\n".to_vec()));
    source.profile.insert(
        PlatformProfileField::Available,
        Some(b"quiet balanced performance\n".to_vec()),
    );

    let report = LinuxHardwareProbe::with_source(source).inspect().unwrap();
    assert_eq!(report.cpu_frequency.availability, Availability::Available);
    let policy = &report.cpu_frequency.policies[0];
    assert_eq!(policy.scaling_driver.as_deref(), Some("intel_pstate"));
    assert_eq!(policy.current_governor.as_deref(), Some("powersave"));
    assert_eq!(
        policy.available_governors,
        vec!["performance", "powersave", "schedutil"]
    );
    assert_eq!(
        policy
            .hardware_frequency_limits
            .as_ref()
            .unwrap()
            .minimum_hz,
        800_000_000
    );
    assert_eq!(
        policy
            .hardware_frequency_limits
            .as_ref()
            .unwrap()
            .maximum_hz,
        4_800_000_000
    );
    assert_eq!(
        policy.policy_frequency_limits.as_ref().unwrap().minimum_hz,
        1_200_000_000
    );
    assert_eq!(
        report.platform_power.current_profile.as_deref(),
        Some("balanced")
    );
    assert_eq!(
        report.snapshot.cpu.unwrap().observed_frequency.availability,
        Availability::Unknown
    );
}

#[test]
fn cpufreq_missing_and_inconsistent_values_are_not_invented() {
    let mut missing = FixtureSource::standard();
    missing.policies = Ok(vec![1]);
    let missing = LinuxHardwareProbe::with_source(missing).inspect().unwrap();
    assert_eq!(missing.cpu_frequency.policies[0].current_governor, None);

    let mut invalid = FixtureSource::standard();
    invalid.policies = Ok(vec![1]);
    invalid.set_frequency_policy(1, CpuFrequencyField::CpuInfoMinFrequencyKhz, b"5000");
    invalid.set_frequency_policy(1, CpuFrequencyField::CpuInfoMaxFrequencyKhz, b"4000");
    invalid.set_frequency_policy(
        1,
        CpuFrequencyField::ScalingMinFrequencyKhz,
        b"18446744073709551615",
    );
    invalid.set_frequency_policy(1, CpuFrequencyField::ScalingMaxFrequencyKhz, b"1000");
    let invalid = LinuxHardwareProbe::with_source(invalid).inspect().unwrap();
    let policy = &invalid.cpu_frequency.policies[0];
    assert_eq!(policy.hardware_frequency_limits, None);
    assert_eq!(policy.policy_frequency_limits, None);
    assert_eq!(invalid.snapshot.cpu.unwrap().observed_frequency.value, None);
}

#[test]
fn cpufreq_absence_is_unavailable_and_not_a_claim_of_change_permission() {
    let report = LinuxHardwareProbe::with_source(FixtureSource::standard())
        .inspect()
        .unwrap();
    assert_eq!(report.cpu_frequency.availability, Availability::Unavailable);
    assert_eq!(
        report.platform_power.availability,
        Availability::Unavailable
    );
}

#[test]
fn partial_subsystem_failures_preserve_other_hardware_and_p011_identity() {
    let mut source = FixtureSource::standard();
    source.cpu_lists.insert(
        CpuListSource::Online,
        Err(DataSourceError::new(DataSourceErrorKind::PermissionDenied)),
    );
    source.meminfo = Ok(b"MemTotal: 8192 kB\nSwapTotal: 0 kB\n".to_vec());
    let report = LinuxHardwareProbe::with_source(source).inspect().unwrap();
    assert_eq!(
        report
            .snapshot
            .platform
            .kernel
            .as_ref()
            .unwrap()
            .version
            .as_deref(),
        Some("6.1.0-fixture")
    );
    assert_eq!(
        report.snapshot.cpu.as_ref().unwrap().topology.availability,
        Availability::Unknown
    );
    assert_eq!(
        report
            .snapshot
            .memory
            .as_ref()
            .unwrap()
            .ram
            .value
            .as_ref()
            .unwrap()
            .total
            .unwrap()
            .0,
        8192 * 1024
    );
    assert!(
        report
            .snapshot
            .diagnostics
            .iter()
            .any(|item| item.provider.as_deref() == Some("linux-cpu-sysfs"))
    );
}

#[test]
fn normalized_snapshot_has_unique_provenance_and_capability_keys() {
    let report = LinuxHardwareProbe::with_source(FixtureSource::standard())
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    let provenance = report
        .snapshot
        .provenance
        .iter()
        .map(|item| item.id.get())
        .collect::<Vec<_>>();
    let mut sorted = provenance.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(provenance.len(), sorted.len());
    let keys = report
        .snapshot
        .capabilities
        .iter()
        .map(|record| (record.id.abi_id(), record.instance.map(|id| id.get())))
        .collect::<Vec<_>>();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort_unstable();
    sorted_keys.dedup();
    assert_eq!(keys.len(), sorted_keys.len());
    assert!(report.snapshot.gpus.is_empty());
    assert!(report.snapshot.displays.is_empty());
}

#[test]
fn synthetic_reference_fixtures_cover_desktop_hybrid_and_laptop_topologies() {
    let mut desktop = FixtureSource::standard();
    desktop.set_cpu_sets(b"0-19\n", b"0-19\n", b"0-19\n");
    for cpu in 0..20 {
        let core = if cpu < 16 { cpu / 2 } else { cpu - 8 };
        desktop.set_topology(cpu, 0, Some(0), core as i64);
    }
    desktop.cpuinfo = Ok(b"model name : Intel Core i7-12700F\n".to_vec());
    desktop.meminfo =
        Ok(b"MemTotal: 33554432 kB\nMemAvailable: 30000000 kB\nSwapTotal: 0 kB\n".to_vec());
    let desktop = LinuxHardwareProbe::with_source(desktop).inspect().unwrap();
    assert_eq!(desktop.cpu_model.as_deref(), Some("Intel Core i7-12700F"));
    assert_eq!(desktop.cpu_topology.unwrap().online_cpu_ids.len(), 20);
    assert_eq!(
        desktop
            .snapshot
            .cpu
            .unwrap()
            .topology
            .value
            .unwrap()
            .physical_core_count,
        Some(12)
    );
    assert_eq!(
        desktop.memory.host_total_bytes,
        Some(32 * 1024 * 1024 * 1024)
    );

    let mut laptop = FixtureSource::standard();
    laptop.set_cpu_sets(b"0-7\n", b"0-7\n", b"0-7\n");
    for cpu in 0..8 {
        laptop.set_topology(cpu, 0, Some(0), (cpu / 2) as i64);
    }
    laptop.cpuinfo = Ok(b"model name : Intel Core i5-1135G7\n".to_vec());
    laptop.meminfo =
        Ok(b"MemTotal: 16777216 kB\nMemAvailable: 15000000 kB\nSwapTotal: 0 kB\n".to_vec());
    let laptop = LinuxHardwareProbe::with_source(laptop).inspect().unwrap();
    assert_eq!(laptop.cpu_model.as_deref(), Some("Intel Core i5-1135G7"));
    assert_eq!(laptop.cpu_topology.unwrap().online_cpu_ids.len(), 8);
    assert_eq!(
        laptop
            .snapshot
            .cpu
            .unwrap()
            .topology
            .value
            .unwrap()
            .physical_core_count,
        Some(4)
    );
    assert_eq!(
        laptop.memory.host_total_bytes,
        Some(16 * 1024 * 1024 * 1024)
    );
}

#[test]
fn live_linux_hardware_smoke_produces_validated_kernel_cpu_and_memory_facts() {
    let report = LinuxHardwareProbe::system()
        .inspect()
        .expect("live Linux hardware probe should retain P011 identity");
    report.snapshot.validate().unwrap();
    assert_eq!(
        report.snapshot.platform.kernel.as_ref().unwrap().name,
        "Linux"
    );
    if let Some(cpu) = &report.cpu_topology {
        assert!(!cpu.online_cpu_ids.is_empty());
        if let Some(physical) = cpu.physical_core_count {
            assert!(physical > 0);
            assert!(physical as usize <= cpu.online_cpu_ids.len());
        }
        if let Some(allowed) = &cpu.process_allowed_cpu_ids {
            assert!(!allowed.is_empty());
        }
    }
    if let Some(total) = report.memory.host_total_bytes {
        assert!(total > 0);
        if let Some(available) = report.memory.host_available_bytes {
            assert!(available <= total);
        }
    }
}

fn cgroup_files(
    cpu_max: &[u8],
    cpuset: Option<&[u8]>,
    memory_max: &[u8],
    memory_current: &[u8],
    memory_swap_max: &[u8],
) -> CgroupV2Files {
    CgroupV2Files {
        cpu_max: Ok(Some(cpu_max.to_vec())),
        cpuset_cpus_effective: Ok(cpuset.map(<[u8]>::to_vec)),
        memory_max: Ok(Some(memory_max.to_vec())),
        memory_current: Ok(Some(memory_current.to_vec())),
        memory_swap_max: Ok(Some(memory_swap_max.to_vec())),
    }
}

fn write(root: &Path, relative: &str, content: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[derive(Clone)]
struct FixtureSource {
    cpuinfo: Result<Vec<u8>, DataSourceError>,
    meminfo: Result<Vec<u8>, DataSourceError>,
    cpu_lists: HashMap<CpuListSource, Result<Vec<u8>, DataSourceError>>,
    topology: HashMap<(u32, CpuTopologyField), Option<i64>>,
    affinity: Result<Vec<u32>, DataSourceError>,
    scheduler: Result<RawSchedulerState, DataSourceError>,
    policies: Result<Vec<u32>, DataSourceError>,
    policy_values: HashMap<(u32, CpuFrequencyField), Option<Vec<u8>>>,
    profile: HashMap<PlatformProfileField, Option<Vec<u8>>>,
    cgroup: Result<CgroupSource, DataSourceError>,
}

impl FixtureSource {
    fn standard() -> Self {
        let mut source = Self {
            cpuinfo: Ok(b"processor : 0\nmodel name : Example CPU\n".to_vec()),
            meminfo: Ok(
                b"MemTotal: 8192 kB\nMemAvailable: 4096 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n"
                    .to_vec(),
            ),
            cpu_lists: HashMap::new(),
            topology: HashMap::new(),
            affinity: Ok(vec![1, 3]),
            scheduler: Ok(RawSchedulerState {
                policy: 0,
                priority: 0,
            }),
            policies: Err(DataSourceError::new(DataSourceErrorKind::NotFound)),
            policy_values: HashMap::new(),
            profile: HashMap::new(),
            cgroup: Ok(CgroupSource::V2(cgroup_files(
                b"max 100000\n",
                Some(b"1,3\n"),
                b"max\n",
                b"1024\n",
                b"0\n",
            ))),
        };
        source.set_cpu_sets(b"0-3\n", b"0-5\n", b"0-7\n");
        for (cpu, core) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            source.set_topology(cpu, 0, Some(0), core);
        }
        source.profile.insert(PlatformProfileField::Current, None);
        source.profile.insert(PlatformProfileField::Available, None);
        source
    }

    fn set_cpu_sets(&mut self, online: &[u8], present: &[u8], possible: &[u8]) {
        self.cpu_lists
            .insert(CpuListSource::Online, Ok(online.to_vec()));
        self.cpu_lists
            .insert(CpuListSource::Present, Ok(present.to_vec()));
        self.cpu_lists
            .insert(CpuListSource::Possible, Ok(possible.to_vec()));
    }

    fn set_topology(&mut self, cpu: u32, package: i64, die: Option<i64>, core: i64) {
        self.topology
            .insert((cpu, CpuTopologyField::PhysicalPackageId), Some(package));
        self.topology.insert((cpu, CpuTopologyField::DieId), die);
        self.topology
            .insert((cpu, CpuTopologyField::CoreId), Some(core));
    }

    fn set_frequency_policy(&mut self, policy: u32, field: CpuFrequencyField, value: &[u8]) {
        self.policy_values
            .insert((policy, field), Some(value.to_vec()));
    }
}

impl LinuxDataSource for FixtureSource {
    fn read_os_release(&self, _: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        Ok(b"ID=arch\nNAME=Arch Linux\nPRETTY_NAME=Arch Linux\n".to_vec())
    }

    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        Ok(RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.1.0-fixture".to_owned(),
            machine: "x86_64".to_owned(),
        })
    }

    fn container_marker_present(
        &self,
        _: endfield_unix_platform_linux::ContainerMarker,
    ) -> Result<bool, DataSourceError> {
        Ok(false)
    }

    fn runtime_directory_path(&self) -> Result<Option<std::path::PathBuf>, DataSourceError> {
        Ok(None)
    }

    fn runtime_directory_metadata(
        &self,
        _: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        Err(DataSourceError::new(DataSourceErrorKind::NotFound))
    }

    fn effective_uid(&self) -> u32 {
        1000
    }
}

impl LinuxHardwareDataSource for FixtureSource {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        self.cpuinfo.clone()
    }

    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        self.meminfo.clone()
    }

    fn read_cpu_list(&self, list: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        self.cpu_lists
            .get(&list)
            .cloned()
            .unwrap_or_else(|| Err(DataSourceError::new(DataSourceErrorKind::NotFound)))
    }

    fn read_cpu_topology_field(
        &self,
        cpu: u32,
        field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        Ok(self.topology.get(&(cpu, field)).copied().flatten())
    }

    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        self.affinity.clone()
    }

    fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
        self.scheduler
    }

    fn enumerate_cpufreq_policies(&self) -> Result<Vec<u32>, DataSourceError> {
        self.policies.clone()
    }

    fn read_cpufreq_policy_field(
        &self,
        policy: u32,
        field: CpuFrequencyField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Ok(self.policy_values.get(&(policy, field)).cloned().flatten())
    }

    fn read_platform_profile_field(
        &self,
        field: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Ok(self.profile.get(&field).cloned().flatten())
    }

    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        self.cgroup.clone()
    }
}
