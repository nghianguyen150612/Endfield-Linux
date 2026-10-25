use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use endfield_unix_domain::{MeasurementOrigin, MeasurementValue};
use endfield_unix_platform_linux::{
    CgroupSource, CgroupV2Files, ContainerMarker, CpuFrequencyField, CpuListSource,
    CpuTopologyField, DataSourceError, DataSourceErrorKind, LinuxCpuMemoryTelemetryProvider,
    LinuxCpuSnapshotContext, LinuxDataSource, LinuxHardwareDataSource, LinuxHardwareProbe,
    MAX_LOGICAL_CPU_OBSERVATIONS, MonotonicClockSession, OsReleaseFile, PlatformProfileField,
    RawKernelIdentity, RuntimeDirectoryMetadata, TelemetryClockFactory, TelemetryInterval,
};
use endfield_unix_telemetry::{
    CollectionCompleteness, CollectionRequest, MetricId, MetricScope, MonotonicTimestamp,
    ObservationState, ObservationTarget, SamplingContextId, SamplingMethod, SnapshotRefId,
    TelemetryProvider, TelemetryUnit,
};

const CLOCK: u64 = 810;
const SAMPLE_CONTEXT: u64 = 910;

#[derive(Clone)]
struct FixtureSource {
    state: Arc<Mutex<FixtureState>>,
}

struct FixtureState {
    cpu_stats: VecDeque<Result<Vec<u8>, DataSourceError>>,
    online_sets: VecDeque<Result<Vec<u8>, DataSourceError>>,
    base_online: Vec<u8>,
    meminfo: Result<Vec<u8>, DataSourceError>,
    cgroup: Result<CgroupSource, DataSourceError>,
    policies: Result<Vec<u32>, DataSourceError>,
    policy_values: HashMap<(u32, CpuFrequencyField), Option<Vec<u8>>>,
    proc_stat_reads: usize,
}

impl FixtureSource {
    fn new(
        stats: impl IntoIterator<Item = Result<Vec<u8>, DataSourceError>>,
        online_sets: impl IntoIterator<Item = Result<Vec<u8>, DataSourceError>>,
    ) -> Self {
        let base_online = b"0,2\n".to_vec();
        Self {
            state: Arc::new(Mutex::new(FixtureState {
                cpu_stats: stats.into_iter().collect(),
                online_sets: online_sets.into_iter().collect(),
                base_online,
                meminfo: Ok(
                    b"MemTotal: 1024 kB\nMemAvailable: 256 kB\nSwapTotal: 128 kB\nSwapFree: 32 kB\n"
                        .to_vec(),
                ),
                cgroup: Ok(cgroup_v2(b"4096")),
                policies: Err(error(DataSourceErrorKind::NotFound)),
                policy_values: HashMap::new(),
                proc_stat_reads: 0,
            })),
        }
    }

    fn with_policy(self, policy: u32, field: CpuFrequencyField, bytes: &[u8]) -> Self {
        self.state
            .lock()
            .unwrap()
            .policy_values
            .insert((policy, field), Some(bytes.to_vec()));
        self
    }

    fn set_policies(&self, policies: Result<Vec<u32>, DataSourceError>) {
        self.state.lock().unwrap().policies = policies;
    }

    fn set_cgroup(&self, cgroup: Result<CgroupSource, DataSourceError>) {
        self.state.lock().unwrap().cgroup = cgroup;
    }

    fn proc_stat_reads(&self) -> usize {
        self.state.lock().unwrap().proc_stat_reads
    }
}

impl LinuxDataSource for FixtureSource {
    fn read_os_release(&self, _file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        Ok(b"ID=arch\nNAME=Arch Linux\nPRETTY_NAME=Arch Linux\n".to_vec())
    }

    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        Ok(RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.12.0-fixture".to_owned(),
            machine: "x86_64".to_owned(),
        })
    }

    fn container_marker_present(&self, _marker: ContainerMarker) -> Result<bool, DataSourceError> {
        Ok(false)
    }

    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        Ok(None)
    }

    fn runtime_directory_metadata(
        &self,
        _path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        Err(error(DataSourceErrorKind::NotFound))
    }

    fn effective_uid(&self) -> u32 {
        1000
    }
}

impl LinuxHardwareDataSource for FixtureSource {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        Err(error(DataSourceErrorKind::Unsupported))
    }

    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        self.state.lock().unwrap().meminfo.clone()
    }

    fn read_proc_stat(&self) -> Result<Vec<u8>, DataSourceError> {
        let mut state = self.state.lock().unwrap();
        state.proc_stat_reads += 1;
        state
            .cpu_stats
            .pop_front()
            .unwrap_or_else(|| Err(error(DataSourceErrorKind::NotFound)))
    }

    fn read_cpu_list(&self, source: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        let mut state = self.state.lock().unwrap();
        match source {
            CpuListSource::Online => state
                .online_sets
                .pop_front()
                .unwrap_or_else(|| Ok(state.base_online.clone())),
            CpuListSource::Present | CpuListSource::Possible => Ok(state.base_online.clone()),
        }
    }

    fn read_cpu_topology_field(
        &self,
        _cpu: u32,
        _field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        Ok(None)
    }

    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        Err(error(DataSourceErrorKind::Unsupported))
    }

    fn scheduler_state(
        &self,
    ) -> Result<endfield_unix_platform_linux::RawSchedulerState, DataSourceError> {
        Err(error(DataSourceErrorKind::Unsupported))
    }

    fn enumerate_cpufreq_policies(&self) -> Result<Vec<u32>, DataSourceError> {
        self.state.lock().unwrap().policies.clone()
    }

    fn read_cpufreq_policy_field(
        &self,
        policy: u32,
        field: CpuFrequencyField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .policy_values
            .get(&(policy, field))
            .cloned()
            .flatten())
    }

    fn read_platform_profile_field(
        &self,
        _field: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Ok(None)
    }

    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        self.state.lock().unwrap().cgroup.clone()
    }
}

#[derive(Clone)]
struct FakeClockFactory {
    times: Vec<u64>,
    wait_calls: Arc<AtomicUsize>,
    domain: u64,
    timestamp_domain: u64,
}

impl FakeClockFactory {
    fn interval_250ms() -> Self {
        Self {
            times: vec![
                100,
                250_000_100,
                250_000_200,
                250_000_300,
                250_000_400,
                250_000_500,
            ],
            wait_calls: Arc::new(AtomicUsize::new(0)),
            domain: CLOCK,
            timestamp_domain: CLOCK,
        }
    }

    fn with_times(times: Vec<u64>) -> Self {
        Self {
            times,
            wait_calls: Arc::new(AtomicUsize::new(0)),
            domain: CLOCK,
            timestamp_domain: CLOCK,
        }
    }

    fn with_mismatched_timestamps() -> Self {
        Self {
            times: vec![100, 200, 300, 400],
            wait_calls: Arc::new(AtomicUsize::new(0)),
            domain: CLOCK,
            timestamp_domain: CLOCK + 1,
        }
    }
}

struct FakeClockSession {
    times: VecDeque<u64>,
    last: u64,
    wait_calls: Arc<AtomicUsize>,
    domain: u64,
    timestamp_domain: u64,
}

impl TelemetryClockFactory for FakeClockFactory {
    type Session = FakeClockSession;

    fn start_session(
        &self,
    ) -> Result<Self::Session, endfield_unix_platform_linux::ClockSessionError> {
        Ok(FakeClockSession {
            times: self.times.iter().copied().collect(),
            last: 0,
            wait_calls: self.wait_calls.clone(),
            domain: self.domain,
            timestamp_domain: self.timestamp_domain,
        })
    }
}

impl MonotonicClockSession for FakeClockSession {
    fn domain(&self) -> endfield_unix_telemetry::ClockDomainId {
        endfield_unix_telemetry::ClockDomainId::new(self.domain).unwrap()
    }

    fn now(
        &mut self,
    ) -> Result<MonotonicTimestamp, endfield_unix_platform_linux::ClockSessionError> {
        self.last = self.times.pop_front().unwrap_or(self.last + 1);
        Ok(MonotonicTimestamp::new(
            endfield_unix_telemetry::ClockDomainId::new(self.timestamp_domain).unwrap(),
            self.last,
        ))
    }

    fn wait(&mut self, _duration: std::time::Duration) {
        self.wait_calls.fetch_add(1, Ordering::Relaxed);
    }
}

fn provider_id() -> SamplingContextId {
    SamplingContextId::new(SAMPLE_CONTEXT).unwrap()
}

fn error(kind: DataSourceErrorKind) -> DataSourceError {
    DataSourceError::new(kind)
}

fn cgroup_v2(current: &[u8]) -> CgroupSource {
    CgroupSource::V2(CgroupV2Files {
        cpu_max: Ok(Some(b"max 100000".to_vec())),
        cpuset_cpus_effective: Ok(Some(b"0,2".to_vec())),
        memory_max: Ok(Some(b"max".to_vec())),
        memory_current: Ok(Some(current.to_vec())),
        memory_swap_max: Ok(Some(b"max".to_vec())),
    })
}

fn snapshot_context(source: &FixtureSource) -> LinuxCpuSnapshotContext {
    let report = LinuxHardwareProbe::with_source(source.clone())
        .inspect()
        .expect("synthetic P012 report");
    LinuxCpuSnapshotContext::from_hardware_report(&report, SnapshotRefId::new(71).unwrap())
        .expect("validated snapshot CPU mapping")
}

fn cpu_stat(aggregate: &str, cpu0: &str, cpu2: &str) -> Vec<u8> {
    format!("cpu {aggregate}\ncpu0 {cpu0}\ncpu2 {cpu2}\n").into_bytes()
}

fn request(metrics: Vec<MetricId>) -> CollectionRequest {
    CollectionRequest::new(provider_id(), metrics, None).unwrap()
}

fn online_reads(count: usize) -> Vec<Result<Vec<u8>, DataSourceError>> {
    (0..count).map(|_| Ok(b"0,2\n".to_vec())).collect()
}

#[test]
fn maps_counter_memory_and_cgroup_sources_into_a_validated_p015_batch() {
    let source = FixtureSource::new(
        [
            Ok(cpu_stat("100 0 0 100 0 0 0 0", "50 0 0 50", "50 0 0 50")),
            Ok(cpu_stat("125 0 0 175 0 0 0 0", "75 0 0 75", "75 0 0 75")),
        ],
        online_reads(4),
    );
    let context = snapshot_context(&source);
    let clock = FakeClockFactory::interval_250ms();
    let wait_calls = clock.wait_calls.clone();
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::from_millis(250).unwrap(),
        clock,
    )
    .with_cpu_snapshot_context(context);
    let metrics = vec![
        MetricId::CpuUtilizationWholeMachine,
        MetricId::CpuUtilizationLogicalCpu,
        MetricId::CpuFrequency,
        MetricId::RamUsed,
        MetricId::RamAvailable,
        MetricId::SwapUsed,
        MetricId::CgroupMemoryUsed,
    ];
    let collection = provider.collect(&request(metrics.clone())).unwrap();
    let batch = collection.batch();
    batch.validate().unwrap();
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    assert_eq!(wait_calls.load(Ordering::Relaxed), 1);
    assert_eq!(batch.observations().len(), 8);
    assert!(
        batch
            .observations()
            .iter()
            .all(|observation| metrics.contains(&observation.metric()))
    );
    let aggregate = find(batch, MetricId::CpuUtilizationWholeMachine, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(
        aggregate.value().unwrap().value(),
        MeasurementValue::Unsigned(2500)
    );
    assert_eq!(aggregate.provenance().method(), SamplingMethod::Derived);
    assert_eq!(
        aggregate
            .temporal()
            .observation()
            .monotonic_observation()
            .unwrap()
            .domain(),
        batch.clock_domain().unwrap()
    );
    assert!(batch.observations().iter().all(|observation| {
        observation
            .temporal()
            .observation()
            .monotonic_observation()
            .is_some_and(|timestamp| timestamp.domain() == batch.clock_domain().unwrap())
    }));
    let cores = batch
        .observations()
        .iter()
        .filter(|observation| observation.metric() == MetricId::CpuUtilizationLogicalCpu)
        .collect::<Vec<_>>();
    assert_eq!(cores.len(), 2);
    assert!(cores.iter().all(|core| core.state().is_measured()));
    assert!(cores.iter().any(|core| matches!(
        core.target(),
        ObservationTarget::LogicalCpu {
            logical_index: 0,
            ..
        }
    )));
    let ram_used = find(batch, MetricId::RamUsed, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(
        ram_used.value().unwrap().value(),
        MeasurementValue::Unsigned(768 * 1024)
    );
    assert!(
        ram_used
            .quality()
            .flags()
            .contains(&endfield_unix_telemetry::QualityFlag::Estimated)
    );
    assert_eq!(ram_used.provenance().origin(), MeasurementOrigin::Derived);
    let available = find(batch, MetricId::RamAvailable, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(
        available.value().unwrap().value(),
        MeasurementValue::Unsigned(256 * 1024)
    );
    let swap = find(batch, MetricId::SwapUsed, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(
        swap.value().unwrap().value(),
        MeasurementValue::Unsigned(96 * 1024)
    );
    let cgroup = find(
        batch,
        MetricId::CgroupMemoryUsed,
        |target| matches!(target, ObservationTarget::Cgroup(context) if context == provider_id()),
    );
    assert_eq!(
        cgroup.value().unwrap().value(),
        MeasurementValue::Unsigned(4096)
    );
    assert_eq!(
        cgroup.provenance().provider().as_str(),
        "linux-cgroup-v2-memory-current"
    );
    assert!(collection.issues().iter().any(|issue| {
        issue.metric() == MetricId::CpuFrequency
            && issue.kind() == endfield_unix_telemetry::ProviderIssueKind::Degraded
    }));
}

#[test]
fn an_aggregate_cpu_sample_survives_per_core_reset_and_unrelated_memory_failure() {
    let source = FixtureSource::new(
        [
            Ok(cpu_stat("100 0 0 100", "80 0 0 20", "50 0 0 50")),
            Ok(cpu_stat("125 0 0 175", "70 0 0 30", "75 0 0 75")),
        ],
        online_reads(3),
    );
    let context = snapshot_context(&source);
    source.state.lock().unwrap().meminfo = Err(error(DataSourceErrorKind::PermissionDenied));
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::from_millis(100).unwrap(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::CpuUtilizationLogicalCpu,
            MetricId::RamAvailable,
        ]))
        .unwrap();
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    let batch = collection.batch();
    let aggregate = find(batch, MetricId::CpuUtilizationWholeMachine, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(
        aggregate.value().unwrap().value(),
        MeasurementValue::Unsigned(2500)
    );
    let cores = batch
        .observations()
        .iter()
        .filter(|observation| observation.metric() == MetricId::CpuUtilizationLogicalCpu)
        .collect::<Vec<_>>();
    assert_eq!(cores.len(), 2);
    assert!(
        cores
            .iter()
            .any(|observation| { matches!(observation.state(), ObservationState::Degraded(None)) })
    );
    assert!(
        cores
            .iter()
            .any(|observation| observation.state().is_measured())
    );
    let memory = find(batch, MetricId::RamAvailable, |target| {
        target == ObservationTarget::Platform
    });
    assert_eq!(memory.state(), ObservationState::PermissionDenied);
    batch.validate().unwrap();
}

#[test]
fn valid_aggregate_and_unaffected_cores_survive_one_malformed_core_record() {
    let source = FixtureSource::new(
        [
            Ok(cpu_stat("100 0 0 100", "50 0 0 50", "50 0 0 50")),
            Ok(b"cpu 125 0 0 175\ncpu0 75 0 0 75\ncpu2 25 x 0 75\n".to_vec()),
        ],
        online_reads(3),
    );
    let context = snapshot_context(&source);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::CpuUtilizationLogicalCpu,
        ]))
        .unwrap();
    let batch = collection.batch();
    let aggregate = find(batch, MetricId::CpuUtilizationWholeMachine, |target| {
        target == ObservationTarget::Platform
    });
    assert!(aggregate.state().is_measured());
    let cores = batch
        .observations()
        .iter()
        .filter(|observation| observation.metric() == MetricId::CpuUtilizationLogicalCpu)
        .collect::<Vec<_>>();
    assert_eq!(cores.len(), 2);
    assert_eq!(
        cores
            .iter()
            .filter(|core| core.state().is_measured())
            .count(),
        1
    );
    assert!(
        cores
            .iter()
            .any(|core| core.state() == ObservationState::MalformedSourceData)
    );
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    batch.validate().unwrap();
}

#[test]
fn memory_observations_survive_an_unavailable_cpu_counter_source() {
    let source = FixtureSource::new(
        [Err(error(DataSourceErrorKind::PermissionDenied))],
        online_reads(1),
    );
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    );
    let collection = provider
        .collect(&request(vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::RamAvailable,
        ]))
        .unwrap();
    let batch = collection.batch();
    assert_eq!(
        find(batch, MetricId::CpuUtilizationWholeMachine, |target| {
            target == ObservationTarget::Platform
        })
        .state(),
        ObservationState::PermissionDenied
    );
    assert!(
        find(batch, MetricId::RamAvailable, |target| target
            == ObservationTarget::Platform)
        .state()
        .is_measured()
    );
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    batch.validate().unwrap();
}

#[test]
fn missing_procfs_source_is_unknown_instead_of_claiming_unsupported_hardware() {
    let source = FixtureSource::new([Err(error(DataSourceErrorKind::NotFound))], online_reads(1));
    source.state.lock().unwrap().meminfo = Err(error(DataSourceErrorKind::NotFound));
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    );
    let collection = provider
        .collect(&request(vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::RamAvailable,
        ]))
        .unwrap();
    assert!(
        collection
            .batch()
            .observations()
            .iter()
            .all(|observation| { observation.state() == ObservationState::Unknown })
    );
    assert!(
        collection
            .batch()
            .observations()
            .iter()
            .all(|observation| observation.state() != ObservationState::Unsupported)
    );
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
}

#[test]
fn zero_duration_cpu_window_is_degraded_and_clock_domain_mismatch_is_rejected() {
    let source = FixtureSource::new(
        [
            Ok(cpu_stat("100 0 0 100", "50 0 0 50", "50 0 0 50")),
            Ok(cpu_stat("125 0 0 175", "75 0 0 75", "75 0 0 75")),
        ],
        online_reads(2),
    );
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::with_times(vec![100, 100, 100, 100]),
    );
    let collection = provider
        .collect(&request(vec![MetricId::CpuUtilizationWholeMachine]))
        .unwrap();
    let aggregate = &collection.batch().observations()[0];
    assert_eq!(aggregate.state(), ObservationState::Degraded(None));
    assert!(aggregate.value().is_none());
    collection.batch().validate().unwrap();

    let source = FixtureSource::new(
        [Ok(cpu_stat("100 0 0 100", "50 0 0 50", "50 0 0 50"))],
        online_reads(1),
    );
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::with_mismatched_timestamps(),
    );
    let error = provider
        .collect(&request(vec![MetricId::CpuUtilizationWholeMachine]))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        endfield_unix_telemetry::ProviderErrorKind::TemporaryFailure
    );
}

#[test]
fn hotplug_invalidates_snapshot_local_logical_cpu_associations_but_keeps_aggregate() {
    let source = FixtureSource::new(
        [
            Ok(cpu_stat("100 0 0 100", "50 0 0 50", "50 0 0 50")),
            Ok(b"cpu 125 0 0 175\ncpu0 60 0 0 60\ncpu1 10 0 0 10\ncpu2 55 0 0 55\n".to_vec()),
        ],
        [
            Ok(b"0,2\n".to_vec()),
            Ok(b"0,2\n".to_vec()),
            Ok(b"0-2\n".to_vec()),
        ],
    );
    let context = snapshot_context(&source);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::from_millis(100).unwrap(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::CpuUtilizationLogicalCpu,
        ]))
        .unwrap();
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    let batch = collection.batch();
    let aggregate = find(batch, MetricId::CpuUtilizationWholeMachine, |target| {
        target == ObservationTarget::Platform
    });
    assert!(aggregate.state().is_measured());
    let core = find(batch, MetricId::CpuUtilizationLogicalCpu, |target| {
        matches!(target, ObservationTarget::Unassociated(_))
    });
    assert_eq!(core.state(), ObservationState::Degraded(None));
    batch.validate().unwrap();
}

#[test]
fn cpu_inventory_overflow_marks_all_per_core_scope_unresolved_without_truncating() {
    let cpu_count = MAX_LOGICAL_CPU_OBSERVATIONS + 1;
    let last_cpu = cpu_count - 1;
    let online = format!("0-{last_cpu}\n").into_bytes();
    let mut stat = String::from("cpu 1 0 0 1\n");
    for cpu in 0..cpu_count {
        stat.push_str(&format!("cpu{cpu} 1 0 0 1\n"));
    }
    let stat = stat.into_bytes();
    let source = FixtureSource::new([Ok(stat.clone()), Ok(stat)], []);
    source.state.lock().unwrap().base_online = online;
    let context = snapshot_context(&source);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::from_millis(100).unwrap(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);

    let collection = provider
        .collect(&request(vec![MetricId::CpuUtilizationLogicalCpu]))
        .unwrap();
    let observations = collection.batch().observations();
    assert_eq!(observations.len(), 1);
    assert!(matches!(
        observations[0].target(),
        ObservationTarget::Unassociated(MetricScope::LogicalCpu)
    ));
    assert!(matches!(
        observations[0].state(),
        ObservationState::Degraded(None)
    ));
    assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
    collection.batch().validate().unwrap();
}

#[test]
fn unrequested_metrics_are_not_read_or_emitted_and_v1_cgroup_is_unsupported() {
    let source = FixtureSource::new([], [Ok(b"0,2\n".to_vec())]);
    source.set_cgroup(Ok(CgroupSource::V1));
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source.clone(),
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    );
    let collection = provider
        .collect(&request(vec![MetricId::RamAvailable]))
        .unwrap();
    assert_eq!(collection.batch().observations().len(), 1);
    assert_eq!(source.proc_stat_reads(), 0);

    let cgroup_source = FixtureSource::new([], [Ok(b"0,2\n".to_vec())]);
    cgroup_source.set_cgroup(Ok(CgroupSource::V1));
    let cgroup_provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        cgroup_source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    );
    let cgroup = cgroup_provider
        .collect(&request(vec![MetricId::CgroupMemoryUsed]))
        .unwrap();
    let observation = &cgroup.batch().observations()[0];
    assert_eq!(observation.state(), ObservationState::Unsupported);
    assert!(matches!(observation.target(), ObservationTarget::Cgroup(_)));
    assert!(
        cgroup
            .issues()
            .iter()
            .any(|issue| issue.kind() == endfield_unix_telemetry::ProviderIssueKind::Unsupported)
    );
}

#[test]
fn cgroup_missing_permission_malformed_and_unsafe_sources_keep_explicit_states() {
    let cases = [
        (
            Ok(CgroupSource::V2(CgroupV2Files {
                cpu_max: Ok(None),
                cpuset_cpus_effective: Ok(None),
                memory_max: Ok(None),
                memory_current: Ok(None),
                memory_swap_max: Ok(None),
            })),
            ObservationState::Unknown,
        ),
        (
            Ok(CgroupSource::V2(CgroupV2Files {
                cpu_max: Ok(None),
                cpuset_cpus_effective: Ok(None),
                memory_max: Ok(None),
                memory_current: Err(error(DataSourceErrorKind::PermissionDenied)),
                memory_swap_max: Ok(None),
            })),
            ObservationState::PermissionDenied,
        ),
        (
            Ok(CgroupSource::V2(CgroupV2Files {
                cpu_max: Ok(None),
                cpuset_cpus_effective: Ok(None),
                memory_max: Ok(None),
                memory_current: Ok(Some(b"not-bytes".to_vec())),
                memory_swap_max: Ok(None),
            })),
            ObservationState::MalformedSourceData,
        ),
        (
            Err(error(DataSourceErrorKind::UnsafeSource)),
            ObservationState::MalformedSourceData,
        ),
    ];
    for (cgroup, expected_state) in cases {
        let source = FixtureSource::new([], [Ok(b"0,2\n".to_vec())]);
        source.set_cgroup(cgroup);
        let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
            source,
            TelemetryInterval::default(),
            FakeClockFactory::interval_250ms(),
        );
        let collection = provider
            .collect(&request(vec![MetricId::CgroupMemoryUsed]))
            .unwrap();
        let observation = &collection.batch().observations()[0];
        assert_eq!(observation.state(), expected_state);
        assert!(matches!(observation.target(), ObservationTarget::Cgroup(_)));
        assert_eq!(collection.completeness(), CollectionCompleteness::Partial);
        collection.batch().validate().unwrap();
    }
}

#[test]
fn current_frequency_requires_full_policy_mapping_and_never_uses_maximum_frequency() {
    let source = FixtureSource::new([], online_reads(2))
        .with_policy(0, CpuFrequencyField::RelatedCpus, b"0")
        .with_policy(0, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"2400000")
        .with_policy(1, CpuFrequencyField::RelatedCpus, b"2")
        .with_policy(1, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"2400000");
    source.set_policies(Ok(vec![0, 1]));
    let context = snapshot_context(&source);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![MetricId::CpuFrequency]))
        .unwrap();
    let frequency = &collection.batch().observations()[0];
    assert_eq!(
        frequency.value().unwrap().value(),
        MeasurementValue::Unsigned(2_400_000_000)
    );
    assert_eq!(frequency.value().unwrap().unit(), TelemetryUnit::Hertz);

    let mismatch = FixtureSource::new([], online_reads(2))
        .with_policy(0, CpuFrequencyField::RelatedCpus, b"0")
        .with_policy(0, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"2400000")
        .with_policy(1, CpuFrequencyField::RelatedCpus, b"2")
        .with_policy(1, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"3000000");
    mismatch.set_policies(Ok(vec![0, 1]));
    let context = snapshot_context(&mismatch);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        mismatch,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![MetricId::CpuFrequency]))
        .unwrap();
    assert!(matches!(
        collection.batch().observations()[0].state(),
        ObservationState::Degraded(None)
    ));
}

#[test]
fn a_valid_optional_frequency_observation_can_complete_a_provider_collection() {
    let source = FixtureSource::new([], online_reads(2))
        .with_policy(0, CpuFrequencyField::RelatedCpus, b"0")
        .with_policy(0, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"1800000")
        .with_policy(1, CpuFrequencyField::RelatedCpus, b"2")
        .with_policy(1, CpuFrequencyField::CpuInfoCurrentFrequencyKhz, b"1800000");
    source.set_policies(Ok(vec![0, 1]));
    let context = snapshot_context(&source);
    let provider = LinuxCpuMemoryTelemetryProvider::with_clock_factory(
        source,
        TelemetryInterval::default(),
        FakeClockFactory::interval_250ms(),
    )
    .with_cpu_snapshot_context(context);
    let collection = provider
        .collect(&request(vec![MetricId::CpuFrequency]))
        .unwrap();
    assert_eq!(collection.completeness(), CollectionCompleteness::Complete);
    assert!(collection.issues().is_empty());
    collection.batch().validate().unwrap();
}

#[test]
fn invalid_sampling_intervals_are_rejected_at_the_provider_boundary() {
    for milliseconds in [0, 99, 2_001] {
        assert!(TelemetryInterval::from_millis(milliseconds).is_err());
    }
    assert_eq!(TelemetryInterval::default().millis(), 250);
}

fn find(
    batch: &endfield_unix_telemetry::TelemetryBatch,
    metric: MetricId,
    target: impl Fn(ObservationTarget) -> bool,
) -> &endfield_unix_telemetry::TelemetryObservation {
    batch
        .observations()
        .iter()
        .find(|observation| observation.metric() == metric && target(observation.target()))
        .expect("matching telemetry observation")
}
