# Installed runtime integrity audit (P026)

The Linux runtime audit crate records and compares a bounded inventory of files in an Endfield-managed installation. It is read-only when auditing. The CLI selects exactly one installation with `endfield-unix runtime audit --installation-id ID`; it does not scan Steam, Heroic, or Lutris directories.

## Baseline and compatibility

P025 installation records remain schema v1 and retain their original strict JSON parser and plan identity. Existing v1 records without `.endfield-runtime-baseline.json` remain valid ownership records. The auditor performs bounded read-only structural inspection, but reports file integrity as **UNVERIFIABLE**; it does not create a baseline from their current contents.

New installs create a schema-v1 baseline in private staging after archive verification, extraction, and structural validation, and before writing the ownership marker or publishing the tree. The baseline binds installation ID, generation, family, build, archive SHA-256, manifest SHA-256, and the P025 trust classification. Sorted entries contain relative paths, types, file sizes, streaming SHA-256 digests, executable and set-ID classifications, and symlink targets. Its canonical entries digest detects malformed or inconsistent inventories. The baseline excludes itself to avoid recursion; `.endfield-installation.json` is separately parsed and validated as a control file. Baselines are written with mode 0600 and no-overwrite creation.

Baseline provenance is a local installation snapshot. User-pinned evidence remains limited trust; publisher-authenticated classification is retained only when P024 provided it. A same-user process can alter both files and local metadata, so this snapshot is not a tamper-proof trust anchor. The optional source path re-verifies a user-supplied archive through P024, then reconstructs an expected inventory using P025 extraction semantics in a private temporary directory. It never persists or enrolls that inventory. CLI-supplied keys are user-pinned and remain limited trust; library callers may supply P024 evidence with a stronger authenticated classification. Neither result establishes game compatibility.

## Audit behavior and limits

Traversal uses Linux directory descriptors, `fstatat(AT_SYMLINK_NOFOLLOW)`, `openat(O_NOFOLLOW)`, and `readlinkat`; it does not recurse through symlinks. Regular files are hashed in 64 KiB chunks and checked for identity, size, and change-time changes. The bounded inventories are compared with a sorted merge. Findings distinguish mismatch, missing, unexpected, type, permission, symlink, unreadable, baseline, and race states. A report retains at most 4,096 finding details; reaching that cap marks the result partial and records the number omitted. The CLI prints at most 100 changed paths and reports further omitted details. Partial scans do not declare unobserved entries missing or pass integrity.

Limits follow P025: 200,000 entries, 4 GiB per file, 16 GiB hashed payload, 4,096 bytes per path, 32 components, 4,096 bytes per link target, 16 MiB aggregate path metadata, and 64 MiB baseline JSON. Same-user writers can still race an audit; no filesystem snapshot isolation is claimed.

```sh
endfield-unix runtime list-managed
endfield-unix runtime audit --installation-id proton-…
endfield-unix runtime audit-source --installation-id proton-… \
  --artifact /absolute/path/runtime.tar.gz --manifest /absolute/path/manifest.json \
  --signature /absolute/path/manifest.sig --trust-key /absolute/path/public-key.bin \
  --provider provider --project project --channel stable --family proton \
  --lineage valve-proton --build build-label --artifact-id runtime --key-id key-id
```

The audit command returns 0 for a complete match, 3 for missing-baseline/unverifiable evidence, and 4 for detected mismatch or incomplete scan. It does not repair, install, delete, or launch anything. Source comparison verifies only the selected artifact and evidence scope.

## Validation status

The runtime-audit and runtime-install crates have deterministic baseline, mutation, symlink, permission, source mismatch, legacy, installation, report-limit, and non-repair tests. The actual application CLI end-to-end flow, full Rust 1.85 workspace/GTK qualification, screenshot upload, and GCC/Clang C17 gates passed in [hosted run 41007250223](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41007250223). The original IPC saturation regression passed in this run; its previously observed `WouldBlock` race remains unresolved. Physical Arch desktop and Dell Vostro audits, real publisher authenticity, and Endfield compatibility remain **UNVERIFIED**. P027 pinning boundaries are unchanged.

## P027 use of audit evidence

Pin set and resolve invoke P025 ownership validation and the P026 bounded baseline audit. List/show do not hash payload files. Resolution requires a complete matching local baseline and exact stored identity; mismatch, unavailable installation, incomplete audit, or changed ownership digest reports stale and blocks fallback. A baseline match is local integrity evidence, not publisher authentication or game compatibility.
