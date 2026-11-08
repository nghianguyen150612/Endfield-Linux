# PROMPT034 implementation manifest

Status: partial implementation; production launch unsupported.

Starting baseline: fb743f209fc7935fa86f05b58cbd380d76f245d3. Remote main was verified at the same commit before the feature branch was created. P033 PR #6 is merged with outstanding acceptance debt.

Implemented in this branch:
- Portable typed launch request, configuration, evidence, blocker, warning, policy, plan, approval, state, result, and process-executor contracts.
- Deterministic plan identifiers and plan-integrity validation that rejects altered plan fields.
- Relative executable path validation and fail-closed handling for unknown runtime, prefix, execution, and dependency evidence.
- Headless game launch plan command using P031, P027, P025/P026/P022, P028, and P029 read-only observations.
- Descriptor-relative executable inspection with O_NOFOLLOW and a bounded PE/COFF signature check.
- GTK preflight form on a worker; Play remains disabled.
- Repository-owned synthetic process fixture tests, separate from production authorization.
- Versioned JSON schema for launch-plan output.

Known unsupported behavior:
- No production process adapter is enabled. UMU plan preparation is not a process invocation contract.
- P028 initialization remains initialization_unsupported; no fake Initialized state is produced.
- P033 acceptance is incomplete, so critical dependency readiness remains unknown and blocking.
- No Endfield executable authenticity check or anti-cheat compatibility evidence exists.
- P019 daemon session tracking is not integrated; process survival, GUI-close handling, and restart recovery remain unverified.
- No P035 work was started.

Validation evidence:
- Genuine Rust 1.85.0 `fmt --check`, workspace/all-targets `check`, workspace/all-targets Clippy with `-D warnings`, workspace build, and locked Cargo metadata validation passed locally. Metadata still reports exactly the two application binaries `endfield-unix` and `endfield-unixd`.
- The serialized locked workspace test suite passed, including P034 portable/CLI/controlled-executor tests and the existing P019/P021-P033, daemon/IPC, storage, domain, diagnostics, and FFI regressions. Serialization uses `--test-threads=1` and a private umask because some existing tests inspect live same-UID processes and filesystem permissions.
- The GTK navigation/resize/close smoke passed under Xvfb and D-Bus after installing the real GTK SVG loader dependency (`librsvg2-common`).
- A real CLI invocation returned status 78 with a blocked plan; its output validated against the Draft 2020-12 schema, reported no process creation, and left the isolated XDG paths absent. The launch diagnostic identifies a blocked preflight.
- GCC C17 ABI tests, Clang C17 ABI tests, and GCC ASan/UBSan tests passed. The P034 changes do not modify the C17 ABI.
- `git diff --check` passed before commit.
- Hosted final-head CI is pending; no hosted result is claimed yet.
- Physical qualification is unverified. No real game was executed.

This manifest does not claim P034 completion or Endfield gameplay compatibility.
