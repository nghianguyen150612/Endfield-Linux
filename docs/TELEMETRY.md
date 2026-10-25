# Normalized telemetry contracts (P015)

P015 adds the library-only crate `endfield-unix-telemetry` at
`crates/telemetry`. It defines reusable telemetry data contracts. It does not
sample the host, start timers, poll in the background, store observations, send
IPC, or display live metrics. Every example in this document and the test suite
is synthetic contract data; none came from Endfield or physical hardware.

## Architecture and dependencies

Future Linux providers feed typed observations into the telemetry contracts,
which reuse P007 domain identities and provenance. Application services may
consume those contracts later. GTK, CLI, the daemon, storage, and the C17
optimizer do not depend on this crate directly today.

```text
Future Linux providers (P016–P018)
                 |
                 v
       endfield-unix-telemetry
                 |
                 v
        endfield-unix-domain (P007)
                 |
                 v
       Future application services
          /                    \
       GUI/CLI            Future C17 bridge
```

The telemetry crate uses Rust 2024 with MSRV 1.85.0, forbids unsafe Rust, and
depends only on `endfield-unix-domain`. It has no GTK, Linux, Vulkan, daemon,
storage, optimizer FFI, or external serialization dependency. Its schema is
an in-memory contract version, independent of the app package, P007 snapshot,
P003 C ABI, P010 IPC, TOML configuration, SQLite, and P008 diagnostics.

P007 remains authoritative for `MeasurementUnit`, signed milli-Celsius,
`MeasurementOrigin`, `Availability`, provenance provider-name validation,
`ProvenanceId`, `InstanceId`, and `SystemSnapshot`. Telemetry metric IDs do not
reuse `CapabilityId`; for example, a GPU inventory capability and a GPU
utilization observation are different facts.

## Metric registry

Metric IDs are stable 16-bit values in a telemetry-only namespace. Assigned IDs
are not reused; unknown numeric IDs are rejected. They are not public C ABI
identifiers. The registry reserves these definitions for later providers:

| ID | Metric | Unit | Kind and scope |
| ---: | --- | --- | --- |
| `0x0101` | Whole-machine CPU utilization | Basis points | Rate, platform |
| `0x0102` | One logical CPU utilization | Basis points | Rate, logical CPU |
| `0x0103` | CPU frequency | Hertz | Gauge, CPU |
| `0x0104` | Process CPU time | Nanoseconds | Counter, process context |
| `0x0105` | CPU temperature | Milli-Celsius | Gauge, CPU |
| `0x0106` | CPU power | Milliwatts | Gauge, CPU |
| `0x0107` | Process CPU utilization | Basis points | Rate, process context |
| `0x0201` | RAM used | Bytes | Gauge, platform |
| `0x0202` | RAM available | Bytes | Gauge, platform |
| `0x0203` | Swap used | Bytes | Gauge, platform |
| `0x0204` | Process memory used | Bytes | Gauge, process context |
| `0x0205` | Cgroup memory used | Bytes | Gauge, known cgroup context |
| `0x0301` | Total-device GPU utilization | Basis points | Rate, GPU |
| `0x0302` | GPU memory used | Bytes | Gauge, GPU |
| `0x0303` | Verified dedicated GPU memory capacity | Bytes | Gauge, GPU |
| `0x0304` | GPU temperature | Milli-Celsius | Gauge, GPU |
| `0x0305` | GPU power | Milliwatts | Gauge, GPU |
| `0x0306` | GPU frequency | Hertz | Gauge, GPU |
| `0x0307` | Explicit GPU engine utilization | Basis points | Rate, GPU engine |
| `0x0401` | Frames per second | Milli-frames per second | Rate, game-session context |
| `0x0402` | Individual frame duration | Nanoseconds | Duration, game-session context |
| `0x0403` | Mean frame duration | Nanoseconds | Derived statistic, game-session context |
| `0x0404` | Cumulative frame count | Count | Counter, game-session context |
| `0x0405` | 99th-percentile frame duration | Nanoseconds | Derived statistic, game-session context |

These definitions do not claim that a provider exists or that every platform
can produce a metric. The GPU engine label is a fixed enum; arbitrary driver
strings are not retained. Process and game-session targets use opaque
caller-assigned context IDs. A process name or game identity is never guessed.

## Units and numeric normalization

The contract maps P007's existing units without changing their values or ABI
IDs: Count, Bytes, Hertz, Pixels, milli-frames per second, milliseconds,
microseconds, nanoseconds, milli-Celsius, milliwatts, millivolts, joules, and
millijoules. A telemetry measurement carries both a typed unit and a typed
integer value; the metric definition validates the pair.

Utilization uses telemetry-only basis points: `0` is 0%, `5000` is 50%, and
`10000` is 100%. Whole-machine, one-logical-CPU, total-device, and per-engine
utilization are separate metrics or target scopes. Process CPU use is
normalized to one logical CPU and may exceed 100%; it has no universal 100%
upper bound. Conversions use checked integer arithmetic. Exact conversions
reject overflow, zero denominators, and non-integral results. The explicit
nearest conversion policy rounds to the nearest integer with half values
rounded upward. Signed values are restricted to signed milli-Celsius metrics;
zero remains an ordinary measured value.

`checked_rate_from_counter` derives milli-frames per second from a Count delta
over nanoseconds. The caller chooses exact or nearest rounding. A zero interval,
incompatible counter unit, and overflow are errors. Counter decreases return
reset or indeterminate outcomes according to caller-provided reset and wrap
evidence; an ambiguous wrap is never converted to a negative workload or a
guessed positive delta.

## Time and observation windows

`WallClockTimestamp` is an optional unsigned nanosecond count since the Unix
epoch. It is for reports and correlation. `MonotonicTimestamp` carries an
elapsed nanosecond count and a nonzero `ClockDomainId`; it is not a Unix time,
and its value is comparable only within that explicit clock domain. Callers
inject timestamps. The library starts no clock, worker, or sampling loop.

An observation can carry a monotonic instant, a bounded monotonic window, a
wall-clock timestamp, or both monotonic and wall-clock time. A window validates
`start <= end` and its duration uses checked subtraction. Zero-length windows
can represent a degenerate timestamp range, but rate observations reject them.
Wall-clock corrections do not affect monotonic durations. Freshness compares
monotonic values only; wall-clock-only observations are not freshness
evaluable.

Temporal metadata keeps the time the source observed the value separate from
collection completion and ingestion. Monotonic timestamps in one observation
and one batch must use the declared clock domain. A later collection or
ingestion timestamp cannot precede the observation or collection timestamp
within that domain. No timestamp is synthesized from the time a provider API
returns.

## Observation states and freshness

`TelemetryObservation` constructors keep source state, value, metric unit,
target, temporal metadata, provenance, quality, and a safe diagnostic together.
Failure strings and arbitrary metadata are not part of the type.

| State | Numeric value | Meaning |
| --- | --- | --- |
| Measured | Required; zero is valid | Source returned a valid value |
| Not yet sampled | None | Sampling has not happened yet |
| Unknown | None | Available evidence does not establish a value or support state |
| Provider missing | None | No provider is configured or available |
| Unsupported | None | Evidence says this metric is unsupported |
| Permission denied | None | Collection was blocked by access policy |
| Temporary failure | None | A collection attempt failed transiently |
| Malformed source data | None | Input could not satisfy the metric contract |
| Degraded | Optional | Partial or impaired value; quality is marked partial |

P007 diagnostic categories are reused where they fit. `SafeDiagnostic` stores
only a fixed category; it does not carry a raw provider error or path. Unknown,
provider-missing, unsupported, and failed collection states remain distinct.

Freshness is an evaluation result, not a destructive update to the observation.
`FreshnessPolicy` requires a caller-selected nonzero maximum age. The evaluator
returns Fresh, Stale, Not evaluable, or Invalid temporal relationship. It uses
the end of an observation window as its age anchor, rejects future observations
conservatively, and never compares unrelated clock domains. Missing values
cannot be marked Fresh. Stale results preserve their historical measured value
and original quality. Different metrics can use different policies; P015 does
not set a universal stale threshold.

## Scope, device references, and provenance

Targets distinguish platform, CPU, logical CPU, GPU, fixed GPU engine, process,
cgroup, game session, and explicitly unassociated observations. A
`DeviceReference` contains a P007 `InstanceId` and a caller-owned
`SnapshotRefId`. `SnapshotContext::from_snapshot` accepts only a validated
P007 snapshot and resolves CPU/GPU identities and logical CPU indexes within
that snapshot. The same `InstanceId` under another `SnapshotRefId` does not
refer to the same device. Unassociated readings remain explicit; enumeration
order is not used to select a GPU.

Every observation carries a `TelemetryProvenance`: a provider ID validated by
P007's provider-name policy, a fixed sampling-method classification, and the
P007 measurement origin. A conversion to P007 `Provenance` requires a caller
allocated `ProvenanceId` and can carry the optional wall-clock timestamp.
Diagnostics store only P007 category codes. The model excludes usernames,
paths, command lines, credentials, serials, GPU UUIDs, container IDs,
environment values, and arbitrary provider error text. P008 remains the only
persistent diagnostic logging/report boundary; telemetry values are not
automatically logged or included in support reports.

## Quality and aggregation metadata

Quality is represented with Complete/Partial status and objective flags for
estimated data, inconsistent source data, insufficient sample intervals,
provider-reported uncertainty, and known staleness. A provider-reported
uncertainty is an absolute error in the metric's unit; no confidence percentage
is invented. Sampling metadata can include interval, source resolution,
observation count, aggregation window, missing sample count, and a fixed
derivation method. Zero intervals, zero resolutions, and overflowing sample
counts are rejected. Derived statistics require at least two observations and
a declared derivation method. P015 does not implement a statistics engine or
claim that one GPU utilization sample predicts sustained game performance.

## Batches, schema, and provider boundary

`TelemetryBatch` carries schema version 1, a sampling-context ID, one optional
clock domain, an optional validated snapshot context, and at most 4096
observations. Empty batches are valid to represent a provider run with no
observations. Provider IDs and all other metadata are bounded or closed enums;
there is no free-form metadata map. The batch normalizes observations in a
deterministic order. It rejects unresolved snapshot references, mixed clock
domains, incompatible status/value combinations, unsupported schema versions,
and duplicates. Duplicate identity is metric, target, provider, and observation
timestamp. Different timestamps and different providers remain separate
observations. No sample is silently discarded or averaged.

P015 provides an in-memory schema only; it adds no Serde representation,
telemetry IPC operation, SQLite table, or serialization compatibility promise.
The independent `TelemetryProvider` boundary declares supported metrics and
collects only with caller-supplied request context. It returns a typed complete
or partial collection, or a typed fatal error. P015 provides no orchestration,
privileged access, async worker, polling schedule, network operation, or Linux
source implementation.

## Synthetic validation and next milestones

Contract fixtures cover whole-machine CPU utilization with a known clock and
interval; measured zero, known, unknown, and permission-restricted RAM states;
two distinct GPUs; positive, zero, and negative Celsius values plus failed
sensor reads; `100000` milli-FPS and an independent `10000000` ns frame; a stale
historical value; and incompatible monotonic domains. Tests use synthetic
snapshots, injected timestamps, and in-memory batches only. They do not probe
hardware, sleep for temporal correctness, access persistent user data, or use
the network.

P016 can add Linux CPU and memory providers against these contracts. P017 can
add GPU providers and P018 can add frame providers. Scheduling, retention,
daemon transport, UI graphs, benchmark workflows, and optimizer consumption
remain future work.
