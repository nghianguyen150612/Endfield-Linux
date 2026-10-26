use endfield_unix_domain::MeasurementValue;
use endfield_unix_platform_linux::{
    FrameLogTelemetryProvider, FrameMetricStatus, FrameRecording, FrameSourceFormat,
    FrameTelemetryResult, MAX_FRAME_INTERVALS, parse_frame_recording,
};
use endfield_unix_telemetry::{
    ClockDomainId, CollectionCompleteness, CollectionRequest, DerivationMethod, DurationNanos,
    MetricId, MetricScope, MonotonicObservation, ObservationState, ObservationTarget,
    SamplingContextId, SamplingMethod, TelemetryProvider, TelemetryUnit,
};

const EVENT_FIXTURE: &[u8] = include_bytes!("fixtures/frame-events-v1.csv");
const MANGOHUD_FIXTURE: &[u8] = include_bytes!("fixtures/mangohud-versioned-v1.csv");

fn all_metrics(context: u64) -> CollectionRequest {
    CollectionRequest::new(
        SamplingContextId::new(context).unwrap(),
        vec![
            MetricId::FramesPerSecond,
            MetricId::FrameTime,
            MetricId::MeanFrameTime,
            MetricId::FrameCount,
            MetricId::P99FrameTime,
        ],
        None,
    )
    .unwrap()
}

fn events(intervals: &[u64], first_timestamp: u64, first_index: u64) -> String {
    let mut csv = String::from("# endfield-frame-events-v1\nframe_index,elapsed_ns\n");
    let mut timestamp = first_timestamp;
    csv.push_str(&format!("{first_index},{timestamp}\n"));
    for (offset, duration) in intervals.iter().enumerate() {
        timestamp += duration;
        csv.push_str(&format!(
            "{},{timestamp}\n",
            first_index + offset as u64 + 1
        ));
    }
    csv
}

fn metric_value(report: &FrameTelemetryResult, metric: MetricId) -> Option<u64> {
    report
        .collection()
        .batch()
        .observations()
        .iter()
        .find(|observation| observation.metric() == metric)
        .and_then(|observation| observation.value())
        .and_then(|measurement| match measurement.value() {
            MeasurementValue::Unsigned(value) => Some(value),
            MeasurementValue::Signed(_) => None,
        })
}

#[test]
fn ideal_100_fps_sequence_builds_a_valid_p015_batch() {
    let csv = events(&[10_000_000; 100], 3_456_789_000, 55);
    let recording = parse_frame_recording(csv.as_bytes(), FrameSourceFormat::FrameEventsV1)
        .expect("generated exact 100 fps timeline");
    let provider = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(801).unwrap());
    assert_eq!(provider.declared_metrics().len(), 5);
    for metric in provider.declared_metrics() {
        assert_eq!(metric.definition().scope, MetricScope::GameSession);
    }
    assert_eq!(provider.declared_metrics()[0].stable_id(), 0x0401);
    assert_eq!(provider.declared_metrics()[4].stable_id(), 0x0405);

    let report = provider.collect_report(&all_metrics(77)).unwrap();
    report.collection().batch().validate().unwrap();
    assert_eq!(report.valid_interval_count(), 100);
    assert_eq!(report.recording_duration_ns(), Some(1_000_000_000));
    assert_eq!(
        metric_value(&report, MetricId::FramesPerSecond),
        Some(100_000)
    );
    assert_eq!(
        metric_value(&report, MetricId::MeanFrameTime),
        Some(10_000_000)
    );
    assert_eq!(
        metric_value(&report, MetricId::P99FrameTime),
        Some(10_000_000)
    );
    assert_eq!(report.p99_status(), FrameMetricStatus::Measured);
    assert_eq!(
        report.collection().completeness(),
        CollectionCompleteness::Partial,
        "FrameCount is unsupported for imported fragments"
    );
    let observations = report.collection().batch().observations();
    assert_eq!(observations.len(), 103);
    assert_eq!(
        observations
            .iter()
            .filter(|observation| observation.metric() == MetricId::FrameTime)
            .count(),
        100
    );
    assert!(observations.iter().all(|observation| {
        observation.target() == ObservationTarget::GameSession(SamplingContextId::new(77).unwrap())
    }));
    let fps = observations
        .iter()
        .find(|observation| observation.metric() == MetricId::FramesPerSecond)
        .unwrap();
    assert_eq!(fps.provenance().method(), SamplingMethod::Derived);
    assert!(matches!(
        fps.temporal().observation().monotonic_observation(),
        Some(MonotonicObservation::Window(window))
            if window.start().elapsed_nanos() == 3_456_789_000
                && window.end().elapsed_nanos() == 4_456_789_000
    ));
    let p99 = observations
        .iter()
        .find(|observation| observation.metric() == MetricId::P99FrameTime)
        .unwrap();
    assert_eq!(
        p99.quality().sampling().unwrap().observation_count(),
        Some(100)
    );
    assert_eq!(
        p99.quality().sampling().unwrap().derivation_method(),
        Some(DerivationMethod::Percentile99)
    );
    assert_eq!(
        p99.quality().sampling().unwrap().aggregation_window(),
        Some(DurationNanos(1_000_000_000)),
        "the statistical window covers the complete first-to-last event span"
    );
}

#[test]
fn maximum_frame_timeline_fits_the_p015_batch_limit_without_truncation() {
    let intervals = [1_000_000; MAX_FRAME_INTERVALS];
    let csv = events(&intervals, 50, 1);
    let recording = parse_frame_recording(csv.as_bytes(), FrameSourceFormat::FrameEventsV1)
        .expect("maximum supported complete timeline");
    let provider = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(805).unwrap());
    let report = provider.collect_report(&all_metrics(78)).unwrap();
    report.collection().batch().validate().unwrap();
    assert_eq!(report.valid_interval_count() as usize, MAX_FRAME_INTERVALS);
    assert_eq!(
        report.collection().batch().observations().len(),
        4096,
        "all frame intervals plus FPS, mean, and P99 are returned"
    );
}

#[test]
fn even_and_uneven_pacing_can_have_the_same_mean_rate() {
    let steady = events(&[10_000_000; 4], 100, 1);
    let uneven = events(&[5_000_000, 15_000_000, 5_000_000, 15_000_000], 100, 1);
    let make_report = |csv: &str, clock| {
        let recording =
            parse_frame_recording(csv.as_bytes(), FrameSourceFormat::FrameEventsV1).unwrap();
        FrameLogTelemetryProvider::new(recording, ClockDomainId::new(clock).unwrap())
            .collect_report(&all_metrics(clock + 1))
            .unwrap()
    };
    let first = make_report(&steady, 901);
    let second = make_report(&uneven, 903);
    assert_eq!(
        metric_value(&first, MetricId::FramesPerSecond),
        Some(100_000)
    );
    assert_eq!(
        metric_value(&second, MetricId::FramesPerSecond),
        Some(100_000)
    );
    assert_eq!(
        metric_value(&first, MetricId::MeanFrameTime),
        metric_value(&second, MetricId::MeanFrameTime)
    );
    assert_ne!(
        first
            .collection()
            .batch()
            .observations()
            .iter()
            .filter(|sample| sample.metric() == MetricId::FrameTime)
            .filter_map(|sample| sample.value())
            .map(|value| match value.value() {
                MeasurementValue::Unsigned(nanos) => nanos,
                MeasurementValue::Signed(_) => 0,
            })
            .collect::<Vec<_>>(),
        second
            .collection()
            .batch()
            .observations()
            .iter()
            .filter(|sample| sample.metric() == MetricId::FrameTime)
            .filter_map(|sample| sample.value())
            .map(|value| match value.value() {
                MeasurementValue::Unsigned(nanos) => nanos,
                MeasurementValue::Signed(_) => 0,
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn gaps_are_preserved_and_partial_statistics_are_degraded() {
    let csv = "# endfield-frame-events-v1\nframe_index,elapsed_ns\n1,10000000\n2,19000000\n4,29000000\n5,39000000\n";
    let recording = parse_frame_recording(csv.as_bytes(), FrameSourceFormat::FrameEventsV1)
        .expect("forward index gap is valid partial source data");
    let provider = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(811).unwrap());
    let report = provider.collect_report(&all_metrics(88)).unwrap();
    assert_eq!(report.valid_interval_count(), 2);
    assert_eq!(
        report.fps_status(),
        FrameMetricStatus::DiscontinuousTimeline
    );
    assert_eq!(metric_value(&report, MetricId::FramesPerSecond), None);
    assert_eq!(
        metric_value(&report, MetricId::MeanFrameTime),
        Some(9_500_000)
    );
    assert_eq!(
        report.mean_status(),
        FrameMetricStatus::DiscontinuousTimeline
    );
    let mean = report
        .collection()
        .batch()
        .observations()
        .iter()
        .find(|observation| observation.metric() == MetricId::MeanFrameTime)
        .unwrap();
    assert!(matches!(mean.state(), ObservationState::Degraded(Some(_))));
    assert_eq!(mean.quality().sampling().unwrap().missing_sample_count(), 1);
    report.collection().batch().validate().unwrap();
}

#[test]
fn one_event_and_two_event_recordings_do_not_overstate_statistics() {
    let single = events(&[], 900, 20);
    let recording =
        parse_frame_recording(single.as_bytes(), FrameSourceFormat::FrameEventsV1).unwrap();
    let report = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(821).unwrap())
        .collect_report(&all_metrics(91))
        .unwrap();
    assert_eq!(report.fps_status(), FrameMetricStatus::InsufficientSamples);
    assert_eq!(report.valid_interval_count(), 0);
    assert_eq!(report.collection().batch().observations().len(), 0);

    let two = events(&[16_666_667], 20_000, 5);
    let recording =
        parse_frame_recording(two.as_bytes(), FrameSourceFormat::FrameEventsV1).unwrap();
    let report = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(823).unwrap())
        .collect_report(&all_metrics(93))
        .unwrap();
    assert_eq!(
        metric_value(&report, MetricId::FramesPerSecond),
        Some(60_000)
    );
    assert_eq!(report.mean_status(), FrameMetricStatus::InsufficientSamples);
    assert_eq!(report.p99_status(), FrameMetricStatus::InsufficientSamples);
    assert_eq!(metric_value(&report, MetricId::FrameCount), None);
}

#[test]
fn mangohud_sampled_rows_do_not_become_a_rate_window_or_all_frames() {
    let recording = parse_frame_recording(MANGOHUD_FIXTURE, FrameSourceFormat::MangoHudCsv)
        .expect("MangoHud fixture");
    let FrameRecording::MangoHud(parsed) = &recording else {
        panic!("expected MangoHud source");
    };
    assert_eq!(parsed.samples().len(), 3);
    assert_eq!(parsed.samples()[1].source_fps_milli(), 30_000);
    let provider = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(831).unwrap());
    let report = provider.collect_report(&all_metrics(97)).unwrap();
    report.collection().batch().validate().unwrap();
    assert_eq!(report.valid_interval_count(), 3);
    assert_eq!(
        report.fps_status(),
        FrameMetricStatus::NoVerifiedObservationWindow
    );
    assert_eq!(metric_value(&report, MetricId::FramesPerSecond), None);
    assert_eq!(
        report.collection().batch().observations().len(),
        4,
        "three sampled FrameTime values and one mean; no FPS, P99, or FrameCount"
    );
    assert_eq!(report.p99_status(), FrameMetricStatus::InsufficientSamples);
}

#[test]
fn mangohud_p99_uses_only_the_100_sampled_frametime_rows() {
    let fixture = String::from_utf8(MANGOHUD_FIXTURE.to_vec()).unwrap();
    let mut csv = fixture.lines().take(7).collect::<Vec<_>>().join("\n");
    csv.push('\n');
    for index in 0..100u64 {
        let slow = index >= 98;
        let (fps, frametime) = if slow { ("10", "100") } else { ("100", "10") };
        let elapsed = (index + 1) * 10_000_000;
        csv.push_str(&format!(
            "{fps},{frametime},{},{}\n",
            ["0"; 13].join(","),
            elapsed
        ));
    }
    let recording = parse_frame_recording(csv.as_bytes(), FrameSourceFormat::MangoHudCsv)
        .expect("100 sampled rows with two slow observations");
    let provider = FrameLogTelemetryProvider::new(recording, ClockDomainId::new(833).unwrap());
    let report = provider.collect_report(&all_metrics(99)).unwrap();
    assert_eq!(
        report.fps_status(),
        FrameMetricStatus::NoVerifiedObservationWindow
    );
    assert_eq!(report.p99_status(), FrameMetricStatus::Measured);
    assert_eq!(
        metric_value(&report, MetricId::P99FrameTime),
        Some(100_000_000)
    );
    let p99 = report
        .collection()
        .batch()
        .observations()
        .iter()
        .find(|observation| observation.metric() == MetricId::P99FrameTime)
        .unwrap();
    assert_eq!(
        p99.quality().sampling().unwrap().observation_count(),
        Some(100)
    );
}

#[test]
fn imports_use_caller_context_and_clock_domains_without_cross_recording_state() {
    let recording_a =
        parse_frame_recording(EVENT_FIXTURE, FrameSourceFormat::FrameEventsV1).unwrap();
    let recording_b = recording_a.clone();
    let first = FrameLogTelemetryProvider::new(recording_a, ClockDomainId::new(841).unwrap())
        .collect(&all_metrics(101))
        .unwrap();
    let second = FrameLogTelemetryProvider::new(recording_b, ClockDomainId::new(842).unwrap())
        .collect(&all_metrics(102))
        .unwrap();
    assert_eq!(
        first.batch().sampling_context(),
        SamplingContextId::new(101).unwrap()
    );
    assert_eq!(
        second.batch().sampling_context(),
        SamplingContextId::new(102).unwrap()
    );
    assert_eq!(
        first.batch().clock_domain(),
        Some(ClockDomainId::new(841).unwrap())
    );
    assert_eq!(
        second.batch().clock_domain(),
        Some(ClockDomainId::new(842).unwrap())
    );
    assert!(first.batch().observations().iter().all(|observation| {
        observation.target() == ObservationTarget::GameSession(SamplingContextId::new(101).unwrap())
    }));
    assert!(second.batch().observations().iter().all(|observation| {
        observation.target() == ObservationTarget::GameSession(SamplingContextId::new(102).unwrap())
    }));
    assert!(first.batch().observations().iter().all(|observation| {
        observation
            .value()
            .is_none_or(|measurement| measurement.unit() == TelemetryUnit::Nanoseconds)
            || observation.metric() == MetricId::FramesPerSecond
    }));
}
