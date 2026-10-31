# P021 runtime provider contracts

P021 establishes portable meanings for Wine and Proton runtime metadata,
provider capabilities, source verification, artifact integrity, compatibility
evidence, ownership, and non-executable launch plans. It does not discover,
download, install, verify installed files, benchmark, or launch runtimes.

## Crate boundary

The library-only Cargo package is crates/runtime, named
endfield-unix-runtime. It uses Rust 2024 and MSRV 1.85.0, forbids unsafe code,
and depends only on serde_json 1.0.145 for deterministic JSON previews. It has
no GTK, libadwaita, Relm4, Linux graphics, process, storage, IPC, C17, or
privileged-helper dependency. P022 and P023 headless application commands
consume these contracts through the separate Linux adapter; the daemon does
not consume the runtime provider.

The crate contains only typed data, bounded validation, negotiation, and
provider trait contracts. Its test providers and identities are synthetic.
There is no filesystem discovery, release-server contact, artifact hashing,
prefix creation, or process execution.

## Runtime families, lineage, and identity

RuntimeFamily distinguishes Wine, Proton, and Unknown. Unknown is a supported
representation for unrecognized providers or builds; it is not mapped to a
known family based on a directory name or version label.

RuntimeLineage can represent upstream Wine, Wine staging, Valve Proton,
GE-Proton, DWProton, another derivative, or unknown lineage. These values
classify a claimed build lineage. They do not establish authentic origin or
prove that the build is installed.

RuntimeIdentity combines family, lineage, provider ID, raw version label,
optional build/channel label, platform and process-architecture constraints,
optional source metadata, optional digest identity, and ownership. Version
labels are bounded and preserved as supplied; they need not follow SemVer and
are not sorted as releases. Unknown versions are never coerced to zero.

RuntimeBuildIdentity captures the fields used for exact compatibility scope.
Two identities with the same version label are not considered the same
verified build without matching digest identity. The comparison does not hash
or inspect files.

Ownership is one of Endfield-managed, external user-managed,
distribution-managed, managed by another launcher, or unknown. Discovery
does not transfer ownership. The policy allows an update or removal only for
an Endfield-managed target and a matching explicit user intent. Unknown and
external ownership are denied. A new managed installation is represented as
a plan target and also requires matching user intent. P021 creates no runtime
ownership manifests.

## Adapters

RuntimeAdapterKind defines four distinct arrangements:

- UmuProton describes a Proton launch through a validated UMU integration.
  Declaring the variant does not assert that UMU exists or is usable. UMU
  discovery and integration are P023.
- VerifiedDirectProton describes direct Proton use only for an exact build
  covered by verified compatibility-test evidence. It is not a generic
  fallback when UMU is absent.
- ManagedWine describes a Wine runtime owned and managed by Endfield-Unix
  under future explicit workflows. Defining the type does not install Wine.
- ExternalWine describes an externally managed Wine installation. Its file
  ownership and update policy remain with the system package manager, user,
  or other runtime manager.

Each adapter has a fixed compatible runtime family. UMU and direct Proton
require Proton. Managed Wine and external Wine require Wine. Managed Wine
requires Endfield ownership; external Wine requires explicit external
ownership. No adapter substitutes system Wine for Proton or infers support
from a directory name.

## Provider declarations and operation outcomes

RuntimeProvider declares supported families, provider availability, and a
bounded set of capabilities. Capability states distinguish supported by the
contract, implemented by the provider, verified usable in the current
environment, unsupported, unknown, and unavailable. A provider's capability
does not mean a runtime is installed.

The trait defines typed boundaries for candidate discovery, identity
inspection, source verification, local integrity assessment, compatibility
assessment, installation planning, and launch-plan preparation. Its public
operation methods check declaration and availability before calling
injectable implementation hooks. The default hooks return explicit errors.
An empty discovery report is valid only when a discovery implementation ran;
it is not the representation of an unsupported or missing provider.

Provider errors distinguish unsupported, unknown, not implemented, provider
unavailable, permission denied, missing metadata, invalid metadata,
verification failure, and incompatible request. Capability negotiation
returns structured outcomes including eligible for further verification,
requires verification, requires user approval, insufficient evidence,
unsupported, unknown, unavailable, invalid metadata, or incompatible
request. Passing static negotiation never proves game compatibility or makes
a plan executable.

Runtime presence is separate from provider availability and capability.
Providers may support Wine or Proton discovery while discovering no
installed candidates. P021 itself performs no discovery.

## Source provenance and verification lifecycle

SourceIdentity records provider, source category, optional project and
release/build identifiers, and an optional claimed address. Source categories
include official upstream release, verified project release, verified
distribution package, user-supplied artifact, external installation, and
unknown origin.

A URL, a GitHub host name, a release tag, a familiar directory, or a plausible
file size is only a source claim. It is not authenticity evidence. P021 does
not contain download URLs or contact release servers.

Source verification states progress through unverified, candidate identified,
source identity established, metadata verified, and artifact integrity
verified. Failed and unsupported verification are separate terminal states.
Constructors enforce prerequisites: authenticated source evidence is required
before metadata can be verified; metadata verification and a digest match
from authenticated metadata are required before the source can claim verified
artifact integrity. Verification timestamps are optional and are stored only
when supplied as evidence; P021 does not invent a verification time.

Source identity, source authenticity, artifact integrity, and compatibility
are separate questions. Authenticity evidence has a typed verification
method and evidence ID. User assertion and local metadata do not establish
authenticity. A hash match sourced from a user-supplied or untrusted manifest
does not establish trusted origin.

## Integrity metadata

Digest supports strict SHA-256 hexadecimal values and a bounded representation
for unsupported algorithms. SHA-256 values must be exactly 64 hexadecimal
characters; they are normalized to lowercase. An integrity assessment
distinguishes a match, mismatch, missing expected digest, missing actual
digest, algorithm mismatch, and unsupported algorithm. An unsupported digest
algorithm cannot produce a successful integrity result. It also retains
artifact identity and the authority claimed for the expected digest.

IntegrityRecord compares supplied digest metadata only. It does not open,
read, or hash an artifact and does not authenticate the manifest or source.
P024 adds authenticated manifests; P026 adds actual installed-file checks.
P021 makes no real artifact verification claim.

## Compatibility evidence

CompatibilityScope binds evidence to exact runtime build identity, adapter,
host platform, process architecture, optional prefix architecture, graphics
API, driver requirements, game build, prefix configuration, and environment
restrictions. Evidence from a different build or scope is rejected instead of
being silently reused.

Evidence strength is one of static capability assessment, provider
documentation, user-reported observation, reproducible local test, or verified
compatibility test. States include unknown, not evaluated, ineligible by
static constraints, insufficient evidence, technically eligible for testing,
supported or unsupported by verified evidence, degraded, and conflicting.
Static eligibility is not a verified compatibility result. User reports and
provider documentation are not promoted to verified results. Contradictory
evidence remains conflicting.

Graphics requirements must be explicit in the request. Vulkan presence alone
does not imply Wine/Proton or game compatibility. Evidence stores safe result
summaries and test provenance but no benchmark/FPS fields or fabricated game
results.

KnownGoodRuntimeReference is a future metadata contract. It requires exact
runtime build identity, provider, matching verified integrity identity,
target game build, profile, verified compatibility evidence, evidence time,
and its scope. It does not select the newest version, change a user's pin, or
add a SQLite table. P027 implements actual runtime selection and pinning.

## LaunchPlan schema v1

LaunchPlan is typed, inspectable, structurally validated data. It includes a
schema version, plan ID, exact runtime identity, adapter selection, executable,
ordered argv, explicit working directory, typed environment changes, optional
prefix reference, mount requirements, output capture bounds, process ownership
policy, risk classification, source-verification state, compatibility
assessment, and structural validation state.

The executable is not concatenated with arguments. Arguments remain native
OS strings in memory, so non-UTF-8 paths and arguments are not lossily
converted. Relative executable paths require an explicit
relative-to-working-directory policy. Working directory, prefix, and mount
paths are explicit references; structural validation performs no existence
check and performs no filesystem mutation. Parent-directory traversal,
empty executable paths, NUL bytes, excessive metadata, invalid environment
names, duplicate environment changes, duplicate mount targets, and unsafe
writable-mount risk classifications are rejected.

Environment changes are typed Set, Remove, or PreserveInherited operations.
An explicitly empty Set value differs from Remove and PreserveInherited.
Variable names and values are bounded; duplicate names are rejected. Secret
classification supports credentials, tokens, account-related values,
sensitive paths, and other secrets.

The deterministic JSON dry run includes schema version, adapter and runtime
identity, ordered arguments, verification and compatibility labels, provider
review state when supplied, and executable_now set to false. Private paths,
classified secrets, and non-UTF-8 display values are omitted or redacted by
default. JSON escaping preserves quotes, whitespace, Unicode, backslashes,
and shell metacharacters as literal argument data. It never emits a shell
script.

Structural validity, provider availability, provider implementation,
source verification, compatibility evidence, risk, and explicit plan-scoped
user approval are separate review gates. Privileged or unsupported risk
cannot become eligible for execution from structural validation or approval.
P021 has no process-spawning API.

Prefix ownership distinguishes Endfield-managed, external user-managed,
other-launcher-managed, and unknown references. Mount requirements are
bounded descriptions with read-only/read-write intent and explicit ownership;
they do not issue mount operations. P028's directory-only prefix management is
implemented separately in [PREFIX-MANAGEMENT.md](PREFIX-MANAGEMENT.md); it
preserves P027 selection and does not provide Wine initialization. P029 remains
backup/recovery.

## Validation and later milestones

The P021 contract tests use only synthetic identities and metadata. They cover
Wine, Proton, GE-Proton and DWProton identities; adapter mismatches; provider
capability and error states; source verification prerequisites; digest
validation and authority; exact compatibility scope and conflict; known-good
metadata; argument and environment safety; native non-UTF-8 behavior;
redaction; ownership; risk; and deterministic dry-run serialization.

P021 does not change the daemon lifecycle, IPC v1.0, systemd user service, C17
ABI 0x00010000, P007 capability ID 50, GTK runtime selection, or P006 SQLite
schema. UMU integration is P023, trusted source manifests P024, acquisition
P025, installed integrity P026, pinning P027, isolated prefix directories
P028, backup/recovery P029, and production launch/session ownership P034–P036. Physical Arch-device Wine
and Proton inventory and Endfield compatibility remain unverified.

## Qualification

P021 passed all three jobs in [hosted CI run 40527116968](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40527116968)
on implementation commit `2fa599d7d4042fb2d72bbc2573045d8b9a9fbc5a`. The
Rust job passed formatting, workspace check and Clippy, metadata assertions,
diagnostics and CLI/display smoke checks, the focused runtime tests, the full
workspace test gate, and the serialized IPC gate. Both independent C17 ABI
jobs passed with GCC and Clang. The GTK smoke artifact is named
`endfield-unix-gtk-smoke`.

Two non-root local workspace runs and the local serial IPC gate reproduced
the existing IPC capacity race documented in [TESTING.md](TESTING.md). The
hosted IPC pass does not resolve or rule out that intermittent race. The
optional real systemd user-manager integration and real Endfield gameplay or
compatibility remain unverified.

## P022 Linux discovery adapter

The portable crate still performs no local discovery. P022 implements a
separate library-only `endfield-unix-runtime-linux` adapter and routes
`endfield-unix diagnose runtimes` through its P021 `RuntimeProvider`,
`DiscoveryReport`, candidate, and identity contracts. Discovery is bounded
and read-only. Candidate structure and claimed lineage do not establish
source authenticity, artifact integrity, launch readiness, or compatibility.
Ownership remains external, manager-owned, distribution-managed, or unknown;
the Linux adapter never creates an Endfield-managed runtime. Root policy,
metadata rules, symlink handling, identity scope, tests, and device validation
are documented in [RUNTIME-DISCOVERY-LINUX.md](RUNTIME-DISCOVERY-LINUX.md).

## P023 UMU adapter

P023 adds bounded `umu-run` discovery and a UMU provider declaration in the
Linux crate. It consumes an explicitly selected P022 Proton candidate and
repeats P022 identity inspection before preparing a P021 plan.
`RuntimeAdapterSelection::new(UmuProton, ...)` enforces that the selected
runtime is Proton; Wine-only identities are rejected. UMU and Proton
identities stay separate. A structurally valid UMU candidate yields
`ImplementedByProvider` for local integration and plan preparation, never
`VerifiedUsable`; runtime execution is not advertised. P023 does not claim
source authenticity, artifact integrity, or compatibility. See
[UMU-INTEGRATION.md](UMU-INTEGRATION.md).

## P024 runtime manifest verification

The library-only offline verifier and headless `diagnose runtime-integrity` command
verify exact-byte Ed25519 signatures, scoped explicit anchors, version/expiry policy,
and actual local artifact size/SHA-256. User-pinned matches remain limited trust
(exit 3), without official publisher authentication. Discovery and UMU remain
unverified; no download, extraction, installation or runtime execution is added.
See [runtime trust manifests](RUNTIME-TRUST-MANIFESTS.md) for schema, CLI flags, evidence mapping,
file-race and rollback limits, synthetic tests, and P025/P026 boundaries.

## P025 runtime installation

P025 provides qualified local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. Hosted Rust/GTK, GCC and Clang qualification passed in [CI run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). It does not establish publisher authenticity or support HTTPS acquisition. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for the workflow and limits.


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.

## P027 persistent selection

P027 persists explicit preferences over exact P025 managed identity or a conservatively bound P022 external candidate. Managed pins preserve provider/build/artifact/manifest/generation/provenance and P025 ownership-record digest. External pins preserve canonical location, source category, ownership observation, lineage and its evidence basis, optional build, structural identity fingerprint, and unknown source trust; the transient candidate ID is not the persistent identity. Selection does not create a LaunchPlan or authorize execution. Adapter preference is separate from provider availability; unknown compatibility remains unverified.

## P028 prefix association

P028 stores P027's exact managed or external runtime reference, adapter preference, selection source, pin identity, and registry revision at directory creation. It revalidates managed references through P025/P026 and external references through P022. A later pin change is reported without migrating the prefix; stale profile pins keep P027's blocking/no-fallback behavior. P028 creates directories only. No P021 `LaunchPlan` is executed, no prefix is initialized, and compatibility remains unverified. See [P028 prefix management](PREFIX-MANAGEMENT.md).
