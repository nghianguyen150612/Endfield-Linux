# Optimizer C ABI v1

## Version and compatibility

The public header is native/optimizer/include/endfield_optimizer.h. ABI version
0x00010000 is major 1, minor 0. The high 16 bits hold the major and the low 16
bits hold the minor. A major mismatch returns status 2 (UNSUPPORTED_ABI).
Versioned structures require at least the current struct_size; larger structures
are accepted so additive fields can be appended in a compatible minor version.
Existing fields and numeric identifiers never change meaning.

Every concrete public structure begins with uint32_t abi_version and uint32_t
struct_size. Fields use fixed-width integers and C natural alignment. The header
uses numeric macros instead of C enums. Pointer/count pairs use uint32_t counts.
A zero count permits a null pointer; a nonzero count requires a nonnull pointer.
C can reject null/count mismatches and unreasonable counts, but portable C
cannot prove that an arbitrary nonnull caller pointer references readable
memory. Callers must provide readable spans for the duration of the call.

## Request records

endfield_optimizer_request contains platform and intensity identifiers,
capability records, and goals. Platform identifiers are Linux 1, macOS 2,
FreeBSD 3, and other/unknown 255. Intensities are L0 through L4 as values 0
through 4.

Each capability uses its P001 identifier and availability value:

- 0 unknown
- 1 unavailable
- 2 available
- 3 degraded or partially available

Unknown and unavailable records carry no measurement. value_kind == 0
(VALUE_NONE) means no numeric measurement; value_kind == 1 carries an unsigned
64-bit measurement and a nonzero unit identifier. A numeric zero is therefore
distinct from missing data. Unit IDs are count 1, bytes 2, hertz 3, pixels 4,
milli-frames per second 5, milliseconds 6, microseconds 7, nanoseconds 8,
milli-Celsius 9, milliwatts 10, millivolts 11, joules 12, and millijoules 13.
Each record also carries an instance ID and optional UTF-8 provenance bytes.
Instance zero denotes a platform-wide record; nonzero values distinguish
devices or sensors within this request. Provenance is length-delimited, at most
4096 bytes, and borrowed only for the planning call. Capability identifiers are
the reserved P001 values 1–3, 10–14, 20–21, 30–32, 40–41, and 50–51; unlisted
identifiers are rejected.

Goal identifiers are target FPS 1, target resolution 2, smoothness 3, visual
quality 4, input latency 5, stability 6, thermal/noise 7, and power efficiency
8. Goal records have an explicit present flag. Target FPS is in milli-frames
per second; target resolution uses width and height in pixels; preference
priorities are from 0 through 1000. Zero is a value, not a missing marker. Each
goal ID may appear at most once in a request.

Requests may contain at most 4096 capability records and 8 goal records. These
bounds limit validation work and are part of ABI 1.0.

## Output and ownership

endfield_optimizer_plan sets a valid output pointer to null before validation.
On success, C allocates an opaque plan set. C owns that allocation and only
endfield_optimizer_plan_set_destroy may release it. Destroying a null handle
returns status 1 (INVALID_ARGUMENT). A nonnull acquired handle must be
destroyed exactly once; using a destroyed handle is invalid.

The current P003 implementation validates the request and returns a valid
empty plan set. plan_count therefore reports zero, and every plan_get index is
out of range and returns status 1. No candidates, FPS predictions, evidence, or
trade-offs are generated. The plan-view structure reserves rank and score,
action and impact arrays, evidence and constraint references, uncertainty, and
rollback metadata for later planner work. If a future successful accessor
returns pointers in that view, they are read-only borrowed storage and expire
when the owning plan set is destroyed.

Plan ranks are one-based. Scores are bounded from 0 through 1,000,000 and are
comparable only among candidates from the same request and engine version;
they are not FPS predictions. Impact dimensions and values use the stable P001
identifiers. Uncertainty values are 0 unknown, 1 low, 2 moderate, and 3 high.
Rollback requirement values are 0 unspecified, 1 not required, and 2 required.
Observation-only is the only assigned action identifier in ABI 1.0; later
actions and constraint identifiers must be assigned without reusing reserved
values. P003 returns no plan view, so it emits none of these values yet.

Rust owns request vectors and strings. The C library borrows their storage only
during endfield_optimizer_plan and retains no caller pointer. The Rust PlanSet
guard owns the opaque C allocation and calls the C destroy function from Drop.
Neither allocator crosses the boundary.

Example safe Rust usage:

    use optimizer_ffi::{Intensity, Optimizer, Platform, Request};

    fn inspect_empty_skeleton() -> Result<u32, optimizer_ffi::Error> {
        let optimizer = Optimizer::new()?;
        let request = Request::new(Platform::Linux, Intensity::L0);
        let plans = optimizer.plan(&request)?;
        plans.count()
    }

The safe API validates requests before marshalling. Its raw FFI module contains
all unsafe Rust declarations and calls. Plan-set wrappers are neither Send nor
Sync; no cross-thread handle guarantee is made.

## Status codes

| Value | Name | Meaning |
| --- | --- | --- |
| 0 | SUCCESS | Call completed |
| 1 | INVALID_ARGUMENT | Null required pointer, malformed record, invalid range, or out-of-range index |
| 2 | UNSUPPORTED_ABI | Major ABI mismatch |
| 3 | UNSUPPORTED_IDENTIFIER | Unknown platform, capability, goal, availability, value kind, or unit |
| 4 | INCOMPATIBLE_CONSTRAINTS | Incompatible or unsafe constraints |
| 5 | RESOURCE_EXHAUSTION | Native allocation failed |
| 6 | INTERNAL_FAILURE | Internal native engine failure |

The ABI uses no C bool, Rust bool, Rust enum, Rust String/Vec, or
compiler-sized integer fields. No panic, exception, I/O, or system mutation
crosses this boundary.
