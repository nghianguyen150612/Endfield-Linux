//! One-shot Linux CPU and memory telemetry provider for the P015 contracts.

use std::collections::BTreeSet;
use std::time::Duration;

use endfield_unix_domain::{DiagnosticCategory, MeasurementOrigin, MeasurementValue};
use endfield_unix_telemetry::{
    ClockDomainId, CollectionRequest, DerivationMethod, DeviceReference, MetricId, MetricScope,
    MonotonicTimestamp, ObservationState, ObservationTarget, ObservationTime, ProviderCollection,
    ProviderError, ProviderErrorKind, ProviderId, ProviderIssue, ProviderIssueKind, QualityFlag,
    QualityMetadata, SafeDiagnostic, SamplingMetadata, SamplingMethod, SnapshotContext,
    SnapshotRefId, TelemetryBatch, TelemetryMeasurement, TelemetryObservation, TelemetryProvenance,
    TelemetryProvider, TelemetryUnit, TemporalMetadata,
};

use crate::cgroup::parse_memory_current;
use crate::cpu_stat::{
    CpuCounters, CpuRecordError, CpuStatParseError, CpuStatSnapshot,
    calculate_cpu_utilization_basis_points, parse_proc_stat,
};
use crate::cpu_topology::parse_cpu_list;
use crate::cpufreq::MAX_CPUFREQ_POLICIES;
use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::hardware_probe::LinuxHardwareReport;
use crate::hardware_sources::{
    CgroupSource, CpuFrequencyField, CpuListSource, LinuxHardwareDataSource,
};
use crate::memory_telemetry::{MemoryValue, collect_memory_telemetry};
use crate::telemetry_clock::{
    ClockSessionError, LinuxTelemetryClockFactory, MonotonicClockSession, TelemetryClockFactory,
};

pub const MIN_TELEMETRY_INTERVAL_MS: u64 = 100;
pub const MAX_TELEMETRY_INTERVAL_MS: u64 = 2_000;
/// Leaves room in the P015 batch for aggregate CPU, frequency, memory, and
/// cgroup observations. Larger inventories omit all per-core values explicitly.
pub const MAX_LOGICAL_CPU_OBSERVATIONS: usize =
    endfield_unix_telemetry::MAX_OBSERVATIONS_PER_BATCH - 16;

const LINUX_PROVIDER_NAME: &str = "linux-cpu-memory";
const PROC_STAT_PROVIDER: &str = "linux-proc-stat";
const PROC_MEMINFO_PROVIDER: &str = "linux-proc-meminfo";
const CGROUP_MEMORY_PROVIDER: &str = "linux-cgroup-v2-memory-current";
const CPUFREQ_PROVIDER: &str = "linux-cpufreq-cpuinfo-current";

const DECLARED_METRICS: [MetricId; 7] = [
    MetricId::CpuUtilizationWholeMachine,
    MetricId::CpuUtilizationLogicalCpu,
    MetricId::CpuFrequency,
    MetricId::RamUsed,
    MetricId::RamAvailable,
    MetricId::SwapUsed,
    MetricId::CgroupMemoryUsed,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryInterval {
    millis: u64,
}

impl TelemetryInterval {
    pub fn from_millis(millis: u64) -> Result<Self, LinuxTelemetryConfigError> {
        if !(MIN_TELEMETRY_INTERVAL_MS..=MAX_TELEMETRY_INTERVAL_MS).contains(&millis) {
            return Err(LinuxTelemetryConfigError::InvalidInterval);
        }
        Ok(Self { millis })
    }

    pub const fn millis(self) -> u64 {
        self.millis
    }

    pub const fn duration(self) -> Duration {
        Duration::from_millis(self.millis)
    }
}

impl Default for TelemetryInterval {
    fn default() -> Self {
        Self { millis: 250 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxTelemetryConfigError {
    InvalidInterval,
    InvalidCpuSnapshotContext,
}

/// P012 snapshot plus the ordered kernel CPU IDs that produced its validated
/// logical CPU count. The mapping is valid only for this snapshot's lifetime;
/// any online-set change during telemetry sampling invalidates all per-core
/// associations rather than renumbering CPUs silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxCpuSnapshotContext {
    snapshot: SnapshotContext,
    cpu_device: DeviceReference,
    online_cpu_ids: Vec<u32>,
}

impl LinuxCpuSnapshotContext {
    pub fn from_hardware_report(
        report: &LinuxHardwareReport,
        reference: SnapshotRefId,
    ) -> Result<Self, LinuxTelemetryConfigError> {
        let facts = report
            .cpu_topology
            .as_ref()
            .ok_or(LinuxTelemetryConfigError::InvalidCpuSnapshotContext)?;
        let cpu_info = report
            .snapshot
            .cpu
            .as_ref()
            .ok_or(LinuxTelemetryConfigError::InvalidCpuSnapshotContext)?;
        let expected_count = cpu_info
            .topology
            .value
            .as_ref()
            .map(|topology| topology.logical_processor_count as usize)
            .ok_or(LinuxTelemetryConfigError::InvalidCpuSnapshotContext)?;
        if facts.online_cpu_ids.is_empty()
            || facts.online_cpu_ids.len() != expected_count
            || facts
                .online_cpu_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(LinuxTelemetryConfigError::InvalidCpuSnapshotContext);
        }
        let snapshot = SnapshotContext::from_snapshot(reference, &report.snapshot)
            .map_err(|_| LinuxTelemetryConfigError::InvalidCpuSnapshotContext)?;
        Ok(Self {
            snapshot,
            cpu_device: DeviceReference::new(reference, cpu_info.instance),
            online_cpu_ids: facts.online_cpu_ids.clone(),
        })
    }

    pub const fn snapshot_context(&self) -> &SnapshotContext {
        &self.snapshot
    }

    pub fn online_cpu_ids(&self) -> &[u32] {
        &self.online_cpu_ids
    }

    pub const fn cpu_device(&self) -> DeviceReference {
        self.cpu_device
    }
}

/// On-demand telemetry provider. `collect` performs a bounded synchronous
/// wait for CPU counters; callers on a GUI path must run it on an owned worker.
pub struct LinuxCpuMemoryTelemetryProvider<S, C = LinuxTelemetryClockFactory> {
    source: S,
    interval: TelemetryInterval,
    clock_factory: C,
    cpu_snapshot: Option<LinuxCpuSnapshotContext>,
    provider_id: ProviderId,
}

impl<S> LinuxCpuMemoryTelemetryProvider<S, LinuxTelemetryClockFactory> {
    pub fn new(source: S, interval: TelemetryInterval) -> Self {
        Self::with_clock_factory(source, interval, LinuxTelemetryClockFactory)
    }
}

impl<S, C> LinuxCpuMemoryTelemetryProvider<S, C> {
    pub fn with_clock_factory(source: S, interval: TelemetryInterval, clock_factory: C) -> Self {
        Self {
            source,
            interval,
            clock_factory,
            cpu_snapshot: None,
            provider_id: ProviderId::new(LINUX_PROVIDER_NAME)
                .expect("static telemetry provider identifier is valid"),
        }
    }

    pub fn with_cpu_snapshot_context(mut self, context: LinuxCpuSnapshotContext) -> Self {
        self.cpu_snapshot = Some(context);
        self
    }

    pub const fn interval(&self) -> TelemetryInterval {
        self.interval
    }
}

impl<S, C> TelemetryProvider for LinuxCpuMemoryTelemetryProvider<S, C>
where
    S: LinuxHardwareDataSource,
    C: TelemetryClockFactory,
{
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn declared_metrics(&self) -> &[MetricId] {
        &DECLARED_METRICS
    }

    fn collect(&self, request: &CollectionRequest) -> Result<ProviderCollection, ProviderError> {
        let requested = request
            .metrics()
            .iter()
            .copied()
            .filter(|metric| DECLARED_METRICS.contains(metric))
            .collect::<BTreeSet<_>>();
        if requested.is_empty() {
            let batch = TelemetryBatch::new(request.context(), None, None, Vec::new())
                .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
            return ProviderCollection::complete(batch)
                .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData));
        }

        let mut clock = self
            .clock_factory
            .start_session()
            .map_err(|_| self.provider_error(ProviderErrorKind::TemporaryFailure))?;
        let domain = clock.domain();
        let cpu_requested = requested.contains(&MetricId::CpuUtilizationWholeMachine)
            || requested.contains(&MetricId::CpuUtilizationLogicalCpu);
        let cpu_pair = if cpu_requested {
            Some(
                sample_cpu_pair(&self.source, self.interval, &mut clock)
                    .map_err(|_| self.provider_error(ProviderErrorKind::TemporaryFailure))?,
            )
        } else {
            None
        };

        let mut observations = Vec::new();
        let mut issues = Vec::new();
        if let Some(pair) = &cpu_pair {
            if requested.contains(&MetricId::CpuUtilizationWholeMachine) {
                observations.push(
                    aggregate_cpu_observation(pair, &mut clock, domain, &mut issues)
                        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?,
                );
            }
            if requested.contains(&MetricId::CpuUtilizationLogicalCpu) {
                add_logical_cpu_observations(
                    pair,
                    self.cpu_snapshot.as_ref(),
                    &mut clock,
                    domain,
                    &mut observations,
                    &mut issues,
                )
                .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
            }
        }

        let memory_requested = requested.iter().any(|metric| {
            matches!(
                metric,
                MetricId::RamAvailable | MetricId::RamUsed | MetricId::SwapUsed
            )
        });
        if memory_requested {
            add_memory_observations(
                &self.source,
                &requested,
                &mut clock,
                domain,
                &mut observations,
                &mut issues,
            )
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        }
        if requested.contains(&MetricId::CgroupMemoryUsed) {
            add_cgroup_observation(
                &self.source,
                request.context(),
                &mut clock,
                domain,
                &mut observations,
                &mut issues,
            )
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        }
        if requested.contains(&MetricId::CpuFrequency) {
            add_cpu_frequency_observation(
                &self.source,
                self.cpu_snapshot.as_ref(),
                &mut clock,
                domain,
                &mut observations,
                &mut issues,
            )
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        }

        let completed = clock
            .now()
            .map_err(|_| self.provider_error(ProviderErrorKind::TemporaryFailure))?;
        ensure_clock_domain(completed, domain)
            .map_err(|_| self.provider_error(ProviderErrorKind::TemporaryFailure))?;
        let snapshot = self
            .cpu_snapshot
            .as_ref()
            .map(|context| context.snapshot_context().clone());
        let batch = TelemetryBatch::new(request.context(), Some(domain), snapshot, observations)
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        if issues.is_empty() {
            ProviderCollection::complete(batch)
                .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))
        } else {
            ProviderCollection::partial(batch, issues)
                .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))
        }
    }
}

impl<S, C> LinuxCpuMemoryTelemetryProvider<S, C> {
    fn provider_error(&self, kind: ProviderErrorKind) -> ProviderError {
        ProviderError::new(self.provider_id.clone(), kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceFailure {
    Unsupported,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
    Degraded,
    Unknown,
}

impl SourceFailure {
    fn from_data_source(error: DataSourceError) -> Self {
        match error.kind {
            DataSourceErrorKind::NotFound => Self::Unknown,
            DataSourceErrorKind::Unsupported => Self::Unsupported,
            DataSourceErrorKind::PermissionDenied => Self::PermissionDenied,
            DataSourceErrorKind::InvalidUtf8
            | DataSourceErrorKind::MalformedData
            | DataSourceErrorKind::TooLarge
            | DataSourceErrorKind::UnexpectedType
            | DataSourceErrorKind::UnsafeSource => Self::MalformedSourceData,
            DataSourceErrorKind::Other => Self::TemporaryFailure,
        }
    }

    const fn state(self) -> ObservationState {
        match self {
            Self::Unsupported => ObservationState::Unsupported,
            Self::PermissionDenied => ObservationState::PermissionDenied,
            Self::TemporaryFailure => ObservationState::TemporaryFailure,
            Self::MalformedSourceData => ObservationState::MalformedSourceData,
            Self::Degraded => ObservationState::Degraded(None),
            Self::Unknown => ObservationState::Unknown,
        }
    }

    const fn issue_kind(self) -> ProviderIssueKind {
        match self {
            Self::Unsupported => ProviderIssueKind::Unsupported,
            Self::PermissionDenied => ProviderIssueKind::PermissionDenied,
            Self::TemporaryFailure => ProviderIssueKind::TemporaryFailure,
            Self::MalformedSourceData => ProviderIssueKind::MalformedSourceData,
            Self::Degraded | Self::Unknown => ProviderIssueKind::Degraded,
        }
    }
}

struct CpuSample {
    stat: Result<CpuStatSnapshot, SourceFailure>,
    online_cpu_ids: Result<Vec<u32>, SourceFailure>,
    at: MonotonicTimestamp,
}

struct CpuPair {
    first: CpuSample,
    second: Option<CpuSample>,
    window: Option<endfield_unix_telemetry::ObservationWindow>,
}

fn sample_cpu_pair<S: LinuxHardwareDataSource, C: MonotonicClockSession>(
    source: &S,
    interval: TelemetryInterval,
    clock: &mut C,
) -> Result<CpuPair, ClockSessionError> {
    let first = capture_cpu_sample(source, clock)?;
    if first.stat.is_err() {
        return Ok(CpuPair {
            first,
            second: None,
            window: None,
        });
    }
    clock.wait(interval.duration());
    let second = capture_cpu_sample(source, clock)?;
    let window = endfield_unix_telemetry::ObservationWindow::new(first.at, second.at).ok();
    Ok(CpuPair {
        first,
        second: Some(second),
        window,
    })
}

fn capture_cpu_sample<S: LinuxHardwareDataSource, C: MonotonicClockSession>(
    source: &S,
    clock: &mut C,
) -> Result<CpuSample, ClockSessionError> {
    let online_cpu_ids = source
        .read_cpu_list(CpuListSource::Online)
        .map_err(SourceFailure::from_data_source)
        .and_then(|bytes| parse_cpu_list(&bytes).map_err(|_| SourceFailure::MalformedSourceData));
    let stat = source
        .read_proc_stat()
        .map_err(SourceFailure::from_data_source)
        .and_then(|bytes| parse_proc_stat(&bytes).map_err(map_proc_stat_parse_error));
    let at = clock.now()?;
    ensure_clock_domain(at, clock.domain()).map_err(|_| ClockSessionError::ElapsedTimeOverflow)?;
    Ok(CpuSample {
        stat,
        online_cpu_ids,
        at,
    })
}

fn map_proc_stat_parse_error(_error: CpuStatParseError) -> SourceFailure {
    SourceFailure::MalformedSourceData
}

fn aggregate_cpu_observation<C: MonotonicClockSession>(
    pair: &CpuPair,
    clock: &mut C,
    domain: ClockDomainId,
    issues: &mut Vec<ProviderIssue>,
) -> Result<TelemetryObservation, endfield_unix_telemetry::TelemetryError> {
    let (state, window) = aggregate_cpu_state(pair, issues);
    let completed = clock
        .now()
        .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
    ensure_clock_domain(completed, domain)?;
    let temporal = cpu_temporal(pair, window, completed)?;
    let quality = cpu_quality(
        window,
        state.is_measured(),
        matches!(state, ObservationState::Degraded(_)),
    )?;
    let provenance = telemetry_provenance(
        PROC_STAT_PROVIDER,
        SamplingMethod::Derived,
        MeasurementOrigin::Derived,
    )?;
    TelemetryObservation::new(
        MetricId::CpuUtilizationWholeMachine,
        ObservationTarget::Platform,
        state,
        temporal,
        provenance,
        quality,
        diagnostic_for_state(state),
    )
}

fn aggregate_cpu_state(
    pair: &CpuPair,
    issues: &mut Vec<ProviderIssue>,
) -> (
    ObservationState,
    Option<endfield_unix_telemetry::ObservationWindow>,
) {
    let window = pair.window;
    let Some(second) = &pair.second else {
        let state = pair
            .first
            .stat
            .as_ref()
            .err()
            .copied()
            .unwrap_or(SourceFailure::Unknown);
        add_issue(
            issues,
            MetricId::CpuUtilizationWholeMachine,
            state.issue_kind(),
        );
        return (state.state(), window);
    };
    let (first_stat, second_stat) = match (&pair.first.stat, &second.stat) {
        (Ok(first), Ok(second)) => (first, second),
        (Err(error), _) | (_, Err(error)) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationWholeMachine,
                error.issue_kind(),
            );
            return (error.state(), window);
        }
    };
    let (first, later) = match (first_stat.aggregate, second_stat.aggregate) {
        (Some(Ok(first)), Some(Ok(later))) => (first, later),
        (Some(Err(_)), _) | (_, Some(Err(_))) | (None, _) | (_, None) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationWholeMachine,
                ProviderIssueKind::MalformedSourceData,
            );
            return (ObservationState::MalformedSourceData, window);
        }
    };
    let Some(window) = window else {
        add_issue(
            issues,
            MetricId::CpuUtilizationWholeMachine,
            ProviderIssueKind::Degraded,
        );
        return (ObservationState::Degraded(None), None);
    };
    if window.is_zero_duration() {
        add_issue(
            issues,
            MetricId::CpuUtilizationWholeMachine,
            ProviderIssueKind::Degraded,
        );
        return (ObservationState::Degraded(None), Some(window));
    }
    match calculate_cpu_utilization_basis_points(first, later) {
        Ok(value) => (
            ObservationState::Measured(TelemetryMeasurement::new(
                MeasurementValue::Unsigned(value),
                TelemetryUnit::BasisPoints,
            )),
            Some(window),
        ),
        Err(_error) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationWholeMachine,
                ProviderIssueKind::Degraded,
            );
            (ObservationState::Degraded(None), Some(window))
        }
    }
}

fn add_logical_cpu_observations<C: MonotonicClockSession>(
    pair: &CpuPair,
    context: Option<&LinuxCpuSnapshotContext>,
    clock: &mut C,
    domain: ClockDomainId,
    observations: &mut Vec<TelemetryObservation>,
    issues: &mut Vec<ProviderIssue>,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    let resolution = resolve_logical_cpus(pair, context);
    match resolution {
        Err(failure) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationLogicalCpu,
                failure.issue_kind(),
            );
            let completed = clock
                .now()
                .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
            ensure_clock_domain(completed, domain)?;
            let temporal = cpu_temporal(pair, pair.window, completed)?;
            let quality =
                partial_cpu_quality(pair.window, matches!(failure, SourceFailure::Degraded))?;
            observations.push(TelemetryObservation::new(
                MetricId::CpuUtilizationLogicalCpu,
                ObservationTarget::Unassociated(MetricScope::LogicalCpu),
                failure.state(),
                temporal,
                telemetry_provenance(
                    PROC_STAT_PROVIDER,
                    SamplingMethod::Derived,
                    MeasurementOrigin::Derived,
                )?,
                quality,
                diagnostic_for_state(failure.state()),
            )?);
            return Ok(());
        }
        Ok(cpu_ids) => {
            let context = context.expect("resolved CPU mapping requires a context");
            let Some(second) = pair.second.as_ref() else {
                return Ok(());
            };
            let first_stat = pair.first.stat.as_ref().expect("resolved stat source");
            let second_stat = second.stat.as_ref().expect("resolved stat source");
            let window = pair.window;
            let completed = clock
                .now()
                .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
            ensure_clock_domain(completed, domain)?;
            let temporal = cpu_temporal(pair, window, completed)?;

            for (logical_index, cpu_id) in cpu_ids.iter().copied().enumerate() {
                let target = ObservationTarget::LogicalCpu {
                    cpu: context.cpu_device(),
                    logical_index: logical_index as u32,
                };
                let first_counter = first_stat.logical_cpus.get(&cpu_id);
                let later_counter = second_stat.logical_cpus.get(&cpu_id);
                let (state, quality) = per_cpu_state(first_counter, later_counter, window, issues)?;
                observations.push(TelemetryObservation::new(
                    MetricId::CpuUtilizationLogicalCpu,
                    target,
                    state,
                    temporal,
                    telemetry_provenance(
                        PROC_STAT_PROVIDER,
                        SamplingMethod::Derived,
                        MeasurementOrigin::Derived,
                    )?,
                    quality,
                    diagnostic_for_state(state),
                )?);
            }
        }
    }
    Ok(())
}

fn resolve_logical_cpus(
    pair: &CpuPair,
    context: Option<&LinuxCpuSnapshotContext>,
) -> Result<Vec<u32>, SourceFailure> {
    let context = context.ok_or(SourceFailure::Unknown)?;
    let second = pair.second.as_ref().ok_or_else(|| {
        pair.first
            .stat
            .as_ref()
            .err()
            .copied()
            .unwrap_or(SourceFailure::Unknown)
    })?;
    let first_online = pair.first.online_cpu_ids.as_ref().map_err(|error| *error)?;
    let second_online = second.online_cpu_ids.as_ref().map_err(|error| *error)?;
    if first_online != second_online || first_online != &context.online_cpu_ids {
        return Err(SourceFailure::Degraded);
    }
    let first_stat = pair.first.stat.as_ref().map_err(|error| *error)?;
    let second_stat = second.stat.as_ref().map_err(|error| *error)?;
    if first_stat.records_exceeded
        || second_stat.records_exceeded
        || first_stat.invalid_identities > 0
        || second_stat.invalid_identities > 0
    {
        return Err(SourceFailure::Degraded);
    }
    let expected = first_online.iter().copied().collect::<BTreeSet<_>>();
    let first_ids = first_stat
        .logical_cpus
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    let second_ids = second_stat
        .logical_cpus
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    if first_ids != expected || second_ids != expected {
        return Err(SourceFailure::Degraded);
    }
    if first_online.len() > MAX_LOGICAL_CPU_OBSERVATIONS {
        return Err(SourceFailure::Degraded);
    }
    Ok(first_online.clone())
}

fn per_cpu_state(
    first: Option<&Result<CpuCounters, CpuRecordError>>,
    later: Option<&Result<CpuCounters, CpuRecordError>>,
    window: Option<endfield_unix_telemetry::ObservationWindow>,
    issues: &mut Vec<ProviderIssue>,
) -> Result<(ObservationState, QualityMetadata), endfield_unix_telemetry::TelemetryError> {
    let (first, later) = match (first, later) {
        (Some(Ok(first)), Some(Ok(later))) => (*first, *later),
        (Some(Err(_)), _) | (_, Some(Err(_))) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationLogicalCpu,
                ProviderIssueKind::MalformedSourceData,
            );
            return Ok((
                ObservationState::MalformedSourceData,
                QualityMetadata::partial(),
            ));
        }
        _ => {
            add_issue(
                issues,
                MetricId::CpuUtilizationLogicalCpu,
                ProviderIssueKind::Degraded,
            );
            return Ok((ObservationState::Unknown, QualityMetadata::partial()));
        }
    };
    let Some(window) = window else {
        add_issue(
            issues,
            MetricId::CpuUtilizationLogicalCpu,
            ProviderIssueKind::Degraded,
        );
        return Ok((ObservationState::Degraded(None), QualityMetadata::partial()));
    };
    if window.is_zero_duration() {
        add_issue(
            issues,
            MetricId::CpuUtilizationLogicalCpu,
            ProviderIssueKind::Degraded,
        );
        return Ok((
            ObservationState::Degraded(None),
            partial_cpu_quality(Some(window), true)?,
        ));
    }
    match calculate_cpu_utilization_basis_points(first, later) {
        Ok(value) => Ok((
            ObservationState::Measured(TelemetryMeasurement::new(
                MeasurementValue::Unsigned(value),
                TelemetryUnit::BasisPoints,
            )),
            cpu_quality(Some(window), true, false)?,
        )),
        Err(_) => {
            add_issue(
                issues,
                MetricId::CpuUtilizationLogicalCpu,
                ProviderIssueKind::Degraded,
            );
            Ok((
                ObservationState::Degraded(None),
                partial_cpu_quality(Some(window), true)?,
            ))
        }
    }
}

fn cpu_temporal(
    pair: &CpuPair,
    window: Option<endfield_unix_telemetry::ObservationWindow>,
    completed: MonotonicTimestamp,
) -> Result<TemporalMetadata, endfield_unix_telemetry::TelemetryError> {
    if let Some(window) = window {
        TemporalMetadata::new(ObservationTime::window(window), Some(completed), None)
    } else {
        TemporalMetadata::new(
            ObservationTime::monotonic(pair.first.at),
            Some(completed),
            None,
        )
    }
}

fn cpu_quality(
    window: Option<endfield_unix_telemetry::ObservationWindow>,
    measured: bool,
    inconsistent: bool,
) -> Result<QualityMetadata, endfield_unix_telemetry::TelemetryError> {
    let Some(window) = window.filter(|window| !window.is_zero_duration()) else {
        return Ok(if measured {
            QualityMetadata::complete()
        } else if inconsistent {
            QualityMetadata::partial().with_flag(QualityFlag::InconsistentSourceData)
        } else {
            QualityMetadata::partial()
        });
    };
    let duration = window.duration()?;
    let sampling = SamplingMetadata::new(
        Some(duration),
        Some(1),
        Some(2),
        Some(duration),
        0,
        Some(DerivationMethod::CounterDelta),
    )?;
    Ok(if measured {
        QualityMetadata::complete().with_sampling(sampling)
    } else if inconsistent {
        QualityMetadata::partial()
            .with_flag(QualityFlag::InconsistentSourceData)
            .with_sampling(sampling)
    } else {
        QualityMetadata::partial().with_sampling(sampling)
    })
}

fn partial_cpu_quality(
    window: Option<endfield_unix_telemetry::ObservationWindow>,
    inconsistent: bool,
) -> Result<QualityMetadata, endfield_unix_telemetry::TelemetryError> {
    cpu_quality(window, false, inconsistent)
}

fn add_memory_observations<S: LinuxHardwareDataSource, C: MonotonicClockSession>(
    source: &S,
    requested: &BTreeSet<MetricId>,
    clock: &mut C,
    domain: ClockDomainId,
    observations: &mut Vec<TelemetryObservation>,
    issues: &mut Vec<ProviderIssue>,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    let bytes = source.read_proc_meminfo();
    let observed_at = clock
        .now()
        .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
    ensure_clock_domain(observed_at, domain)?;
    match bytes {
        Err(error) => {
            let failure = SourceFailure::from_data_source(error);
            for metric in [
                MetricId::RamAvailable,
                MetricId::RamUsed,
                MetricId::SwapUsed,
            ] {
                if requested.contains(&metric) {
                    add_issue(issues, metric, failure.issue_kind());
                    observations.push(memory_failure_observation(metric, failure, observed_at)?);
                }
            }
        }
        Ok(bytes) => match collect_memory_telemetry(&bytes) {
            Err(_error) => {
                for metric in [
                    MetricId::RamAvailable,
                    MetricId::RamUsed,
                    MetricId::SwapUsed,
                ] {
                    if requested.contains(&metric) {
                        add_issue(issues, metric, ProviderIssueKind::MalformedSourceData);
                        observations.push(memory_failure_observation(
                            metric,
                            SourceFailure::MalformedSourceData,
                            observed_at,
                        )?);
                    }
                }
            }
            Ok(facts) => {
                push_memory_value(
                    observations,
                    issues,
                    requested,
                    MetricId::RamAvailable,
                    facts.ram_available_bytes,
                    observed_at,
                    false,
                )?;
                push_memory_value(
                    observations,
                    issues,
                    requested,
                    MetricId::RamUsed,
                    facts.estimated_ram_used_bytes,
                    observed_at,
                    true,
                )?;
                push_memory_value(
                    observations,
                    issues,
                    requested,
                    MetricId::SwapUsed,
                    facts.swap_used_bytes,
                    observed_at,
                    false,
                )?;
            }
        },
    }
    Ok(())
}

fn push_memory_value(
    observations: &mut Vec<TelemetryObservation>,
    issues: &mut Vec<ProviderIssue>,
    requested: &BTreeSet<MetricId>,
    metric: MetricId,
    value: MemoryValue,
    at: MonotonicTimestamp,
    estimated: bool,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    if !requested.contains(&metric) {
        return Ok(());
    }
    let target = ObservationTarget::Platform;
    let temporal = point_temporal(at, at)?;
    let provenance = memory_metric_provenance(metric)?;
    let (state, quality) = match value {
        MemoryValue::Valid(value) => (
            ObservationState::Measured(TelemetryMeasurement::new(
                MeasurementValue::Unsigned(value),
                TelemetryUnit::Bytes,
            )),
            if estimated {
                QualityMetadata::complete().with_flag(QualityFlag::Estimated)
            } else {
                QualityMetadata::complete()
            },
        ),
        MemoryValue::Missing => {
            add_issue(issues, metric, ProviderIssueKind::Degraded);
            (ObservationState::Unknown, QualityMetadata::partial())
        }
        MemoryValue::Invalid(error) => {
            add_issue(issues, metric, ProviderIssueKind::MalformedSourceData);
            let quality = if error == crate::memory::MemInfoFieldError::InconsistentValues {
                QualityMetadata::partial().with_flag(QualityFlag::InconsistentSourceData)
            } else {
                QualityMetadata::partial()
            };
            (ObservationState::MalformedSourceData, quality)
        }
    };
    let quality = quality.with_sampling(SamplingMetadata::new(
        None,
        Some(1024),
        Some(1),
        None,
        0,
        None,
    )?);
    observations.push(TelemetryObservation::new(
        metric,
        target,
        state,
        temporal,
        provenance,
        quality,
        diagnostic_for_state(state),
    )?);
    Ok(())
}

fn memory_failure_observation(
    metric: MetricId,
    failure: SourceFailure,
    at: MonotonicTimestamp,
) -> Result<TelemetryObservation, endfield_unix_telemetry::TelemetryError> {
    let state = failure.state();
    TelemetryObservation::new(
        metric,
        ObservationTarget::Platform,
        state,
        point_temporal(at, at)?,
        telemetry_provenance(
            PROC_MEMINFO_PROVIDER,
            memory_metric_method(metric),
            memory_metric_origin(metric),
        )?,
        QualityMetadata::partial(),
        diagnostic_for_state(state),
    )
}

fn add_cgroup_observation<S: LinuxHardwareDataSource, C: MonotonicClockSession>(
    source: &S,
    context: endfield_unix_telemetry::SamplingContextId,
    clock: &mut C,
    domain: ClockDomainId,
    observations: &mut Vec<TelemetryObservation>,
    issues: &mut Vec<ProviderIssue>,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    let source_result = source.cgroup_source();
    let observed_at = clock
        .now()
        .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
    ensure_clock_domain(observed_at, domain)?;
    let value = match source_result {
        Err(error) => Err(SourceFailure::from_data_source(error)),
        Ok(CgroupSource::None | CgroupSource::V1) => Err(SourceFailure::Unsupported),
        Ok(CgroupSource::V2(files)) => match files.memory_current {
            Ok(Some(bytes)) => {
                parse_memory_current(&bytes).map_err(SourceFailure::from_data_source)
            }
            Ok(None) => Err(SourceFailure::Unknown),
            Err(error) => Err(SourceFailure::from_data_source(error)),
        },
    };
    let target = ObservationTarget::Cgroup(context);
    let temporal = point_temporal(observed_at, observed_at)?;
    let provenance = telemetry_provenance(
        CGROUP_MEMORY_PROVIDER,
        SamplingMethod::OperatingSystemInterface,
        MeasurementOrigin::Raw,
    )?;
    let (state, quality) = match value {
        Ok(bytes) => (
            ObservationState::Measured(TelemetryMeasurement::new(
                MeasurementValue::Unsigned(bytes),
                TelemetryUnit::Bytes,
            )),
            QualityMetadata::complete().with_sampling(SamplingMetadata::new(
                None,
                Some(1),
                Some(1),
                None,
                0,
                None,
            )?),
        ),
        Err(failure) => {
            add_issue(issues, MetricId::CgroupMemoryUsed, failure.issue_kind());
            (failure.state(), QualityMetadata::partial())
        }
    };
    observations.push(TelemetryObservation::new(
        MetricId::CgroupMemoryUsed,
        target,
        state,
        temporal,
        provenance,
        quality,
        diagnostic_for_state(state),
    )?);
    Ok(())
}

fn add_cpu_frequency_observation<S: LinuxHardwareDataSource, C: MonotonicClockSession>(
    source: &S,
    context: Option<&LinuxCpuSnapshotContext>,
    clock: &mut C,
    domain: ClockDomainId,
    observations: &mut Vec<TelemetryObservation>,
    issues: &mut Vec<ProviderIssue>,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    let value = match context {
        None => Err(SourceFailure::Unknown),
        Some(context) => sample_verified_current_frequency(source, context),
    };
    let observed_at = clock
        .now()
        .map_err(|_| endfield_unix_telemetry::TelemetryError::MissingTimestamp)?;
    ensure_clock_domain(observed_at, domain)?;
    let target = context.map_or(
        ObservationTarget::Unassociated(MetricScope::Cpu),
        |context| ObservationTarget::Cpu(context.cpu_device()),
    );
    let temporal = point_temporal(observed_at, observed_at)?;
    let provenance = telemetry_provenance(
        CPUFREQ_PROVIDER,
        SamplingMethod::OperatingSystemInterface,
        MeasurementOrigin::Normalized,
    )?;
    let (state, quality) = match value {
        Ok(hz) => (
            ObservationState::Measured(TelemetryMeasurement::new(
                MeasurementValue::Unsigned(hz),
                TelemetryUnit::Hertz,
            )),
            QualityMetadata::complete(),
        ),
        Err(failure) => {
            add_issue(issues, MetricId::CpuFrequency, failure.issue_kind());
            (failure.state(), QualityMetadata::partial())
        }
    };
    observations.push(TelemetryObservation::new(
        MetricId::CpuFrequency,
        target,
        state,
        temporal,
        provenance,
        quality,
        diagnostic_for_state(state),
    )?);
    Ok(())
}

fn sample_verified_current_frequency<S: LinuxHardwareDataSource>(
    source: &S,
    context: &LinuxCpuSnapshotContext,
) -> Result<u64, SourceFailure> {
    let online = source
        .read_cpu_list(CpuListSource::Online)
        .map_err(SourceFailure::from_data_source)
        .and_then(|bytes| parse_cpu_list(&bytes).map_err(|_| SourceFailure::MalformedSourceData))?;
    if online != context.online_cpu_ids {
        return Err(SourceFailure::Degraded);
    }
    let policies = source
        .enumerate_cpufreq_policies()
        .map_err(SourceFailure::from_data_source)?;
    if policies.is_empty() {
        return Err(SourceFailure::Unsupported);
    }
    if policies.len() > MAX_CPUFREQ_POLICIES {
        return Err(SourceFailure::Degraded);
    }
    let mut seen_policies = BTreeSet::new();
    let mut covered = BTreeSet::new();
    let online_set = online.iter().copied().collect::<BTreeSet<_>>();
    let mut frequencies = BTreeSet::new();

    for policy in policies {
        if !seen_policies.insert(policy) {
            return Err(SourceFailure::MalformedSourceData);
        }
        let related = source
            .read_cpufreq_policy_field(policy, CpuFrequencyField::RelatedCpus)
            .map_err(SourceFailure::from_data_source)?;
        let cpu_ids = match related {
            Some(bytes) => {
                parse_cpu_list(&bytes).map_err(|_| SourceFailure::MalformedSourceData)?
            }
            None => source
                .read_cpufreq_policy_field(policy, CpuFrequencyField::AffectedCpus)
                .map_err(SourceFailure::from_data_source)?
                .ok_or(SourceFailure::Unsupported)
                .and_then(|bytes| {
                    parse_cpu_list(&bytes).map_err(|_| SourceFailure::MalformedSourceData)
                })?,
        };
        let relevant = cpu_ids
            .into_iter()
            .filter(|cpu| online_set.contains(cpu))
            .collect::<Vec<_>>();
        if relevant.is_empty() {
            continue;
        }
        for cpu in relevant {
            if !covered.insert(cpu) {
                return Err(SourceFailure::Degraded);
            }
        }
        let current = source
            .read_cpufreq_policy_field(policy, CpuFrequencyField::CpuInfoCurrentFrequencyKhz)
            .map_err(SourceFailure::from_data_source)?
            .ok_or(SourceFailure::Unsupported)?;
        frequencies.insert(parse_current_frequency_hz(&current)?);
    }
    let online_after = source
        .read_cpu_list(CpuListSource::Online)
        .map_err(SourceFailure::from_data_source)
        .and_then(|bytes| parse_cpu_list(&bytes).map_err(|_| SourceFailure::MalformedSourceData))?;
    if online_after != online {
        return Err(SourceFailure::Degraded);
    }
    if covered != online_set || frequencies.is_empty() {
        return Err(SourceFailure::Degraded);
    }
    if frequencies.len() != 1 {
        return Err(SourceFailure::Degraded);
    }
    Ok(*frequencies
        .iter()
        .next()
        .expect("frequency set is nonempty"))
}

fn parse_current_frequency_hz(bytes: &[u8]) -> Result<u64, SourceFailure> {
    if bytes.len() > 4096 {
        return Err(SourceFailure::MalformedSourceData);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SourceFailure::MalformedSourceData)?;
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SourceFailure::MalformedSourceData);
    }
    let khz = text
        .parse::<u64>()
        .map_err(|_| SourceFailure::MalformedSourceData)?;
    khz.checked_mul(1000)
        .filter(|hz| *hz > 0)
        .ok_or(SourceFailure::MalformedSourceData)
}

fn point_temporal(
    observed_at: MonotonicTimestamp,
    completed_at: MonotonicTimestamp,
) -> Result<TemporalMetadata, endfield_unix_telemetry::TelemetryError> {
    TemporalMetadata::new(
        ObservationTime::monotonic(observed_at),
        Some(completed_at),
        None,
    )
}

fn telemetry_provenance(
    provider: &str,
    method: SamplingMethod,
    origin: MeasurementOrigin,
) -> Result<TelemetryProvenance, endfield_unix_telemetry::TelemetryError> {
    TelemetryProvenance::new(ProviderId::new(provider)?, method, origin)
}

fn memory_metric_method(metric: MetricId) -> SamplingMethod {
    match metric {
        MetricId::RamAvailable => SamplingMethod::OperatingSystemInterface,
        MetricId::RamUsed | MetricId::SwapUsed => SamplingMethod::Derived,
        _ => SamplingMethod::OperatingSystemInterface,
    }
}

fn memory_metric_origin(metric: MetricId) -> MeasurementOrigin {
    match metric {
        MetricId::RamAvailable => MeasurementOrigin::Normalized,
        MetricId::RamUsed | MetricId::SwapUsed => MeasurementOrigin::Derived,
        _ => MeasurementOrigin::Normalized,
    }
}

fn memory_metric_provenance(
    metric: MetricId,
) -> Result<TelemetryProvenance, endfield_unix_telemetry::TelemetryError> {
    telemetry_provenance(
        PROC_MEMINFO_PROVIDER,
        memory_metric_method(metric),
        memory_metric_origin(metric),
    )
}

fn diagnostic_for_state(state: ObservationState) -> Option<SafeDiagnostic> {
    let category = match state {
        ObservationState::NotYetSampled | ObservationState::Unknown => {
            DiagnosticCategory::NotYetProbed
        }
        ObservationState::ProviderMissing => DiagnosticCategory::MissingProvider,
        ObservationState::Unsupported => DiagnosticCategory::UnsupportedCapability,
        ObservationState::PermissionDenied => DiagnosticCategory::PermissionDenied,
        ObservationState::TemporaryFailure => DiagnosticCategory::TemporaryFailure,
        ObservationState::MalformedSourceData => DiagnosticCategory::MalformedSourceData,
        ObservationState::Degraded(_) => DiagnosticCategory::StaleObservation,
        ObservationState::Measured(_) => return None,
    };
    Some(SafeDiagnostic::new(category))
}

fn add_issue(issues: &mut Vec<ProviderIssue>, metric: MetricId, kind: ProviderIssueKind) {
    let issue = ProviderIssue::new(metric, kind);
    if !issues.contains(&issue) {
        issues.push(issue);
    }
}

fn ensure_clock_domain(
    timestamp: MonotonicTimestamp,
    domain: ClockDomainId,
) -> Result<(), endfield_unix_telemetry::TelemetryError> {
    if timestamp.domain() != domain {
        Err(endfield_unix_telemetry::TelemetryError::ClockDomainMismatch)
    } else {
        Ok(())
    }
}
