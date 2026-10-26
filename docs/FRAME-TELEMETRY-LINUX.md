# Linux frame telemetry import (P018)

P018 reads one user-selected frame recording through the existing
`endfield-unix-platform-linux` library and maps supported observations into
P015 batches. It does not capture a running game. It does not require GTK, a
daemon, a GPU, a display connection, or process enumeration.

## Supported source formats

The format must be selected explicitly. File extensions are ignored.

| CLI format | Version/header | Input semantics | Data P018 can use |
| --- | --- | --- | --- |
| `frame-events-v1` | P018 canonical v1 | Timestamped events from one source timeline | Frame intervals, measured event rate, mean, P99 when sufficiently sampled |
| `mangohud-csv` | MangoHud legacy header or `log_versioning=v1` header | Periodic CSV rows with the latest frame interval, instantaneous reciprocal FPS, and elapsed logging time | Sampled frame intervals, their mean, sample P99 when sufficiently sampled; no recording-wide FPS |

MangoHud compatibility was checked against upstream commit
[`a68858a15456cc24a08075e20a8c4e40367b2dbd`](https://github.com/flightlessmango/MangoHud/commit/a68858a15456cc24a08075e20a8c4e40367b2dbd):
the [`README.md` FPS logging and `log_interval` descriptions](https://github.com/flightlessmango/MangoHud/blob/a68858a15456cc24a08075e20a8c4e40367b2dbd/README.md),
[`src/logging.cpp` CSV headers, row serialization, and elapsed-time writes](https://github.com/flightlessmango/MangoHud/blob/a68858a15456cc24a08075e20a8c4e40367b2dbd/src/logging.cpp),
and [`src/overlay.cpp` frametime/FPS assignments](https://github.com/flightlessmango/MangoHud/blob/a68858a15456cc24a08075e20a8c4e40367b2dbd/src/overlay.cpp).
P018 recognizes the upstream 16-column frame header beginning
`fps,frametime,cpu_load,...,elapsed`, the seven-column system header, the
unversioned layout, and the versioned `v1` layout. The package-version header
value and system-information row are validated as text and discarded. The
summary CSV, future header versions, missing columns, reordered schemas, and
other historical/future layouts are unsupported or malformed; no format is
guessed.

The committed MangoHud files are source-backed schema fixtures, not files
captured from MangoHud or gameplay. They exercise both header variants without
claiming a tested MangoHud binary version. No physical MangoHud log or actual
Endfield recording was available for this milestone.

### Canonical event file

The canonical file is UTF-8 CSV with this exact version line and header:

```csv
# endfield-frame-events-v1
frame_index,elapsed_ns
1,10000000
2,20000000
3,30000000
```

Each row is one timestamped frame event on the declared source timeline.
`elapsed_ns` is a nonnegative nanosecond timestamp relative to that source;
the first timestamp may be nonzero. `frame_index` is a positive one-based
index and may also start at a non-one value. The source contract defines these
events as consecutive frame completion/presentation events. The parser
validates the declared data, but it cannot authenticate how a user-created
file obtained those events. It does not equate these intervals with GPU render
time, CPU execution time, input-to-display latency, scan-out time, or refresh
period.

Timestamps and indexes must increase strictly. Duplicate or backward values,
zero adjacent intervals, numeric overflow, invalid UTF-8, unexpected columns,
and truncated records fail. A forward index gap is preserved as a known
missing-record count; P018 emits only intervals between adjacent indexes and
does not fill the gap. A recording with a gap has no single recording-wide FPS
observation. It can still report statistics over its valid observed intervals
as degraded data.

### MangoHud timing meaning

Upstream writes `frametime` in milliseconds from the most recent
frame-to-frame presentation interval; P018 converts its decimal value to
nanoseconds, rounding to the nearest nanosecond with ties upward. Upstream
writes `elapsed` as nanoseconds from the beginning of logging. The CSV does not
include the configured `log_interval`. MangoHud can write on every frame when
the interval is zero or periodically when it is nonzero, and the same header
is used. P018 therefore labels all parsed rows as sampled records. It never
claims one CSV row represents every frame. Known row timestamps are retained,
but no sampling interval is attributed to the source.

In the checked upstream implementation the CSV `fps` field is the instantaneous
reciprocal of the latest frametime, not the rate measured across the whole
recording. P018 retains and counts those source values in its parsed typed
recording, but does not convert them into the P015 `FramesPerSecond` metric.
That metric requires a verified observation window. The selected file alone
does not establish that its rows include every frame, so `log_interval` is not
used as an FPS window and no missing frames are inferred.

The reported mean and P99 for MangoHud are over the available sampled
frametime values only. They do not describe unsampled frames. P99 is not
converted to 1% low FPS. MangoHud summary/benchmark CSVs are not accepted.

## P015 metrics and statistical rules

P018 keeps the existing metric IDs and units: `FramesPerSecond` (`0x0401`,
milli-FPS), `FrameTime` (`0x0402`, ns), `MeanFrameTime` (`0x0403`, ns),
`FrameCount` (`0x0404`, count), and `P99FrameTime` (`0x0405`, ns). All targets
are `ObservationTarget::GameSession` using the caller-assigned import-local
sampling context. P015 schemas and validators are unchanged.

- **FPS:** canonical event timelines use `N × 1,000,000,000 / elapsed_ns`,
  where `N` is the count of verified adjacent frame events and `elapsed_ns` is
  the first-to-last observed event span. Milli-FPS uses checked `u128`
  intermediates and round-to-nearest, ties upward. At least two events and a
  nonzero window are required. MangoHud FPS samples are not used for this
  metric.
- **FrameTime:** canonical values are the checked difference between adjacent
  event timestamps and are emitted only when frame indexes are consecutive.
  MangoHud values are its source-reported latest frame interval, converted from
  decimal milliseconds. Positive durations are required.
- **MeanFrameTime:** arithmetic mean over at least two compatible observed
  intervals. Summation and division use checked integers, with nearest
  rounding and half ties upward. P015 sampling metadata records the actual
  observation count and aggregation window.
- **P99FrameTime:** nearest-rank P99, `sorted[ceil(99*N/100)-1]`, with checked
  integer rank arithmetic. At least 100 valid intervals are required. Slow
  frames remain in the input distribution. P99 is not 1% low FPS.
- **FrameCount:** never emitted for imported recordings. A row count or a
  fragment-local frame index does not establish the cumulative counter origin
  or game-session lifetime required by P015.

P015 measured results use the import-local clock domain and retain the
recording-relative timestamps. Historical data has no source wall-clock time;
filesystem modification time is ignored. Collection and ingestion times stay
unknown rather than being presented as frame-observation time. No batch mixes
clock domains or sampling contexts. A full CLI request returns a partial
collection when a metric is unsupported or has insufficient evidence.

Frame-pacing quality is limited to objective facts: observed interval count,
known index gaps, unknown MangoHud row coverage, source timing precision, and
insufficient P99 count. P018 does not assign a smoothness score or diagnose a
driver fault from a long frame.

## CLI and input safety

```sh
endfield-unix diagnose telemetry frames --input ./frames.csv
endfield-unix diagnose telemetry frames \
  --format frame-events-v1 --input ./frames.csv
endfield-unix diagnose telemetry frames \
  --format mangohud-csv --input ./mangohud.csv
```

The default format is `frame-events-v1`; MangoHud must be selected explicitly.
The command reads only the named file, emits locally, and performs no GUI
startup, daemon request, process scan, game launch, instrumentation, storage
write, network request, or telemetry persistence. The printed import-local
session association does not prove Endfield identity, a running process, a
GPU, or full session coverage. File paths and MangoHud system/game metadata
are never printed or placed in telemetry records.

The open file descriptor must refer to a regular file. Linux opens reject a
final-component symlink and use nonblocking mode so FIFOs cannot hang the
command. Reads are streaming, capped at 64 MiB, and each logical CSV record is
capped at 64 KiB and 64 fields. At most 4,093 frame intervals or MangoHud
records are accepted so exact frame samples and requested summaries fit the
4,096-observation P015 batch. Numeric fields and metadata rows are never
evaluated or executed.

Exit status `0` means a valid complete or partial report, `2` means invalid
arguments, `70` means malformed source or fatal provider failure, `74` means
input/output I/O failure, and `78` means the explicitly selected source
variant is unsupported. No failure is reported as a measured zero.

Example deterministic fixture invocation and output fields:

```sh
endfield-unix diagnose telemetry frames \
  --format frame-events-v1 \
  --input crates/platform-linux/tests/fixtures/frame-events-v1.csv
```

The fixture reports two valid 10 ms intervals, 100.000 FPS, a 10 ms mean, and
an unavailable P99 because the sample is smaller than 100 intervals. This is
synthetic contract data, not an Endfield performance measurement.

## Validation status and manual check

Parser, statistics, P015 provider, path-safety, and real-binary file-based
tests are deterministic and run without a GPU or game. The MangoHud fixture
checks compatibility against upstream source semantics but is not an
upstream-generated runtime recording. Follow-up physical validation requires
the user to select an actual recording:

```sh
cargo +1.85.0 run --locked -p endfield-unix -- \
  diagnose telemetry frames \
  --format mangohud-csv \
  --input /path/to/recording.csv
```

No private directory is searched automatically. Real Endfield frame capture,
Arch RTX 5070 frame telemetry, and Dell Vostro frame telemetry remain
**UNVERIFIED**. P019 remains responsible for stronger live game-session
ownership; P018 does not start capture or associate recordings with a process.
