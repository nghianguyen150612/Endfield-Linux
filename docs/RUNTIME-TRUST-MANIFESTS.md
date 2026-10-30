# Runtime source manifests (P024)

`endfield-unix-runtime-verification` is a Rust 2024/MSRV 1.85 library. It verifies
explicit local metadata and artifact files offline. It never downloads, extracts,
installs, executes, enrolls keys, writes checkpoints, or changes discovered runtimes.
No production publisher keys are bundled. Synthetic fixtures are not Proton releases.

## Schema v1

See the complete [synthetic manifest](../crates/runtime-verification/tests/fixtures/manifest.json).
All fields shown are required. JSON objects reject duplicate and unknown fields at
both levels, including unknown critical fields. Limits: 262144 input bytes, 128
artifacts, 96 ASCII bytes per identifier (`A-Z a-z 0-9 - _ .`), 255 bytes per filename,
and 64 GiB per artifact. JSON recursion protection remains enabled; the typed schema
accepts only the documented shallow structure. Parsing never writes files.

Schema version must equal 1. Manifest versions are integers 1 through 2^63-1.
Runtime family is `wine` or `proton`; Wine lineages are `upstream-wine`,
`wine-staging`, `custom`; Proton lineages are `valve-proton`, `ge-proton`,
`dw-proton`, `custom`. Lineage is signed metadata, not inferred publisher identity.
Build, manifest, project, provider, channel, key and artifact identities are explicit.
Artifact IDs and filenames are unique. Only SHA-256 is supported, encoded as 64
lowercase hexadecimal digits. Missing/invalid digests fail; alternative digest
fields are rejected. Negative, overflowing and oversized lengths fail.
Filenames permit ASCII alphanumerics, hyphen, underscore and dot; leading/trailing
dots, any `..`, separators, NUL, drive syntax and Windows reserved device stems fail.
No filenames are used to open additional files. The caller selects the file.

`issued_at` and `expires_at` use 20-byte UTC RFC3339 timestamps without fractional
seconds, from the Unix epoch onward, with issue strictly before expiry. Future issue
is rejected with zero clock tolerance; expiration is exclusive. CLI policy trusts the
host system clock. Library callers inject time; unknown time is explicit and prevents
P021 trusted artifact promotion and checkpoint export. Key validity intervals also
require trusted time. A trusted clock is the caller's responsibility.

## Exact signature contract

The detached signature is exactly 64 raw Ed25519 bytes. The public key file is
exactly 32 raw Ed25519 public-key bytes, not PEM or hexadecimal. The signing input is:

```text
ASCII("ENDFIELD-UNIX-RUNTIME-MANIFEST-V1") || 0x00 || original_manifest_bytes
```

There is no JSON canonicalization. Whitespace, field order and trailing newlines are
covered verbatim. `ed25519-dalek 2.2.0` uses `VerifyingKey::verify_strict`; weak and
malformed keys, invalid signature lengths and wrong signatures fail. The signed
`key_id` and `signature_algorithm` (`ed25519` only) bind policy selection. The
algorithm does not authenticate the identity of a public key on its own.
SHA-256 is implemented by `sha2 0.10.9`, not custom crypto or OpenSSL subprocesses.

## Trust policy and source states

Callers supply previously authenticated/pinned `TrustAnchor` policies. Each key is
restricted to an exact provider, project, channel, family and lineage. At most 32
anchors are accepted; duplicate key IDs fail. This is a single-signature protocol;
there is no threshold, delegation or automatic rotation protocol. Revoked anchors
are rejected. Rotation requires independently authenticated caller policy changes.
Optional key validity intervals are checked against caller time. No manifest can
enroll its own anchor. Scope applies to every signed artifact; artifact namespaces
are not separately delegated.

`IndependentlyAuthenticatedPublisher` is an explicit caller attestation about key
provenance. Acquire and authenticate publisher keys through an independently
verified channel and retain scope/provenance outside the untrusted artifact source.
A URL, GitHub tag, familiar filename or key beside an archive is insufficient.
The CLI cannot assert this provenance. `--trust-key` means `UserPinned` only.
`Unverified` keys are rejected. No official identity is guessed.

After signature, scope, freshness, file-size and digest success, independently
authenticated policy maps to P021 `VerifiedProjectRelease`, `AuthenticityProof`
(detached signature), `MetadataVerification::Verified`, authenticated manifest digest
authority, `IntegrityAssessment::DigestMatches`, and `ArtifactIntegrityVerified`.
Evidence IDs include the SHA-256 of exact signed metadata. P021 constructors remain
unchanged. User-pinned matches retain `CandidateIdentified`, claimed user assertion,
`NotVerified` metadata and `UserSupplied` digest authority. Mathematical signature
validity is reported separately and never silently becomes publisher authenticity.

`ArtifactInput` carries a selected opened file, verified manifest token (private
construction), artifact ID, filename and build. IDs, filename and build must match
the signed entry; project/provider/channel/family/lineage are bound by anchor scope.
Expected size and digest always come from that entry. `VerificationReport` exposes
P021 `SourceMetadata` and `IntegrityAssessment`. Its request adapters return only
already verified evidence after matching request identity/expected authority.
P021 requests alone contain no artifact bytes. `VerifiedArtifactProvider` implements
the P021 provider trait for one verified input and rejects mismatched requests.
The verifier exposes VerifySource
and VerifyIntegrity capabilities; it is not an installation/discovery provider and
does not add those capabilities to P022 or UMU. No P021 public API was loosened.

## Versions, rollback and offline limits

Versions are globally monotonic within provider/project/channel/family/lineage scope,
including when manifest ID or build changes. `TrustedCheckpoint` is an injectable
persistence boundary: caller attests its origin and secure retention. Version zero,
invalid scope, rollback and same-version different identity/content fail. Increasing
versions pass. Checkpoints bind exact manifest SHA-256, not reserialized JSON.
The library can export a checkpoint only after authenticated policy and trusted time;
it never persists it. No SQLite migration or trust-store mutation exists.
Without a supplied checkpoint, status is `NoCheckpoint`. Separate invocations have
no anti-rollback guarantee unless authenticated state is retained externally.
Even a valid checkpoint and nonexpired metadata cannot prove a newer release has
not been withheld: freshness is not fully established offline. This is inspired by
TUF root/version/expiration principles, **not TUF compliance**. It does not implement
Sigstore certificates, OIDC identities, transparency evidence or Sigstore verification.

## Artifact safety and limitations

Files are opened read-only once. Linux uses O_NOFOLLOW and O_NONBLOCK to reject final
symlinks and avoid FIFO blocking before regular-file checks. Parent symlinks are
allowed. Other platforms reject path opening conservatively; a caller-owned regular
file handle can be supplied. Hashing seeks to zero and streams 64 KiB chunks with
checked counters and a 64 GiB maximum. Actual bytes and length are compared to the
signed entry. Handle metadata is checked before/after; Linux device, inode, length,
mtime and ctime (including nanoseconds) detect observable changes. Bounded metadata
reads use the same file policy. I/O, permission, type, size, checksum and changed-file
failures are typed fixed categories without logging paths or manifest contents.

This detects observable changes, not every adversarial in-place modification during
hashing. A path can be replaced after opening; the result describes the opened handle.
Stronger workflows need immutable owned staging and revalidation in P025. A verified
archive is not a verified extracted tree (P026), healthy Proton, satisfied Wine
dependencies, Endfield compatibility, or authorization to replace an installation.
External Steam/Heroic/Lutris/system runtimes and P023 prerequisites remain unchanged.

## Read-only CLI and Arch procedure

Use an independently selected key and explicit scope. For the synthetic fixture:

```sh
cargo +1.85.0 build --locked -p endfield-unix
scripts/runtime-integrity-smoke.sh
```

For a local artifact (all four file arguments must be absolute):

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose runtime-integrity \
  --manifest /absolute/path/manifest.json --signature /absolute/path/manifest.sig \
  --artifact /absolute/path/runtime.tar.gz --trust-key /absolute/path/public-key.bin \
  --key-id selected-key --provider selected-provider --project selected-project \
  --channel stable --family proton --lineage ge-proton --build selected-build \
  --artifact-id selected-artifact
```

Choose scope/build/ID independently; do not infer trust from manifest claims.
The basename of the selected artifact must equal its signed filename. CLI exits:
0 reserved for authenticated-policy verification (currently library only), 3 local
match under user pin, 4 signature/key failure, 5 digest/size mismatch, 6 schema,
identity, time or rollback policy failure, 7 filesystem/I/O failure, 8 unsupported
signature algorithm, 2 invalid CLI syntax. Exit 3 is deliberately not shell success.
No GTK initialization, daemon, runtime process, network or persistent mutation occurs.
Do not automatically include these files in support reports.

## Evidence and references

Deterministic test-only key seed `[42;32]` is public test material, never a production
key. Tests cover valid/schema-negative manifests, duplicate/unknown fields, bounds,
wrong keys and scope, tampered bytes and whitespace, domain misuse, revoked/unverified
anchors, trusted checkpoint conflicts/rollback, time policy, multi-chunk and empty
files, corruption, size and identity mismatch, symlinks, FIFOs, devices and permissions.
Hosted CI executes the actual binary on committed signed fixtures and rejects a
whitespace-tampered manifest, preserving existing Rust/GTK, IPC, daemon/session,
storage/domain/telemetry, metadata and GCC/Clang ABI gates. Qualification evidence is
recorded in [TESTING.md](TESTING.md); tests are synthetic, not publisher validation.

References consulted 2026-10-29:

- [TUF specification 1.0.37](https://theupdateframework.github.io/specification/v1.0.37/), modified 2026-10-08: root trust, version tracking, expiry, freeze limitations.
- [TUF metadata roles](https://theupdateframework.io/docs/metadata/).
- [Sigstore verification](https://docs.sigstore.dev/cosign/verifying/verify/): identity/issuer and evidence requirements, not implemented here.
- [Valve Proton](https://github.com/ValveSoftware/Proton/tree/bc896217ffcd67f892f952b010f898fe7bd0f0a1): Steam distribution does not supply this project's trust anchor.
- [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom/tree/107d34ec7c562c56448ab59f13b3f2d88d056e81): project warns against unrelated distribution sites.
- [UMU](https://github.com/Open-Wine-Components/umu-launcher/tree/e2b203a1fdd2af9f35166f5713cb3f85d72587e2): integration is separate from source authenticity.

Official Proton release authenticity: UNVERIFIED

Official GE-Proton release authenticity: UNVERIFIED

Official DWProton release authenticity: UNVERIFIED

Arch desktop artifact verification: UNVERIFIED

Dell Vostro artifact verification: UNVERIFIED

Real Endfield compatibility: UNVERIFIED

P025 consumes P024 evidence for explicitly approved local artifact installation with controlled staging. User-pinned evidence remains limited trust and is labeled as such; authenticated publisher evidence requires an independently established caller policy and rollback checkpoint. HTTPS acquisition is unavailable because the signed v1 manifest has no authenticated URL. P026 owns installed-file audits.

## P025 runtime installation

P025 provides qualified local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. Hosted Rust/GTK, GCC and Clang qualification passed in [CI run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). It does not establish publisher authenticity or support HTTPS acquisition. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for the workflow and limits.


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.
