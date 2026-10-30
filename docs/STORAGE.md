# Local storage foundation

P006 adds the `endfield-unix-storage` Rust library crate. It owns local configuration, user-directory resolution, SQLite initialization, schema migration, and typed storage errors. It is synchronous and has no GTK, C optimizer, runtime, daemon, or network dependency. GUI integrations must call it from a worker so filesystem and SQLite waits do not block GTK's main thread.

## Linux directories

The application namespace is `endfield-unix`:

| Data | XDG base | Default when unset or invalid | Application path |
| --- | --- | --- | --- |
| Configuration | `XDG_CONFIG_HOME` | `$HOME/.config` | `endfield-unix/config.toml` |
| State/database | `XDG_DATA_HOME` | `$HOME/.local/share` | `endfield-unix/state.sqlite3` |
| Cache | `XDG_CACHE_HOME` | `$HOME/.cache` | `endfield-unix/` |

Each XDG override must be absolute and contain no `..` component; an invalid override uses that directory's XDG default. If a default is needed, `HOME` must resolve to an absolute safe path or discovery returns `HomeUnavailable`. All three valid XDG overrides can be used without resolving `HOME`. No repository or working-directory fallback exists. `AppPaths::from_linux_environment` and `AppPaths::from_base_dirs` allow deterministic injection. The Linux resolver is isolated behind `AppPaths`; macOS and FreeBSD native conventions remain future work.

Discovery itself creates nothing. `Storage::initialize` explicitly creates the config and data directories. App-owned directories are restricted to mode `0700` on Unix. The cache path is resolved but is not created until a future cache service needs it.

## TOML schema v1

The configuration version is independent of package version `0.1.0`:

```toml
schema_version = 1

[ui]
theme = "system"
language = "system"
```

Defaults are defined by Rust `Default` implementations. Supported theme values are `system`, `light`, and `dark`; supported language values are `system` and `en`. Missing `[ui]`, `theme`, or `language` fields receive their centralized defaults. A missing schema version, wrong TOML type, or malformed TOML is an error; unsupported schema versions and invalid enum values have distinct typed errors. Unknown fields are rejected so a load/save cannot silently discard data this version does not understand. There is no fabricated migration from a pre-v1 released format.

The GTK shell does not yet edit or consume these preferences. A stored `dark` preference, for example, is data only and is not reported as applied. Telemetry, credentials, runtime pins, profiles, and speculative domain tables are not in this schema.

`ConfigStore::load` returns validated defaults without writing when the file is missing. An explicit `initialize` or `save` creates it. Read failures, permission failures, malformed TOML, invalid values, and unsupported versions are reported separately. Failure does not reset or overwrite the old file.

## Atomic config replacement and writers

Saving validates and serializes the complete document before creating directories or touching the destination. Writers then take an exclusive advisory lock on the persistent `config.toml.lock` sidecar, compare the exact loaded bytes with the current file, and return `ConcurrentUpdate` if another writer has committed since the document was loaded. This compare-and-swap also protects first-run initialization. Callers reload and reapply their intended edit after a conflict.

For a matching revision, the writer creates a unique mode-`0600` temporary file in the config directory, writes all bytes, flushes and synchronizes the file, and renames it over the destination. The parent directory is synchronized when supported. Failed pre-rename writes remove the temporary file and preserve the previous config. If parent-directory synchronization fails after rename, the new file may already be visible; the save reports the error and the caller should reload before retrying. The implementation does not claim power-loss durability on filesystems that cannot provide file and directory synchronization.

The lock is advisory and coordinates writers using this crate on filesystems whose locking semantics support it. Revision checking detects external edits observed before replacement, but a program that ignores the lock can still race in the final check-to-rename interval. Application directories, final-component symlinks, and special files are refused where platform APIs allow that check; parent directories supplied by XDG may themselves follow normal filesystem resolution.

## SQLite initialization and migrations

`Database::initialize` explicitly creates `state.sqlite3`; `Database::open` refuses a missing database. Each connection enables foreign keys, uses a two-second SQLite busy timeout, and is protected by a Rust mutex. The API exposes metadata operations and schema information, not the raw connection. Logical metadata writes use transactions and parameterized SQL.

Initialization requests WAL and uses the journal mode SQLite actually reports. SQLite may report a non-WAL mode on a filesystem that does not support WAL; the library records and exposes that selected mode. Opening an existing supported database does not force a journal-mode change. While a WAL database is open, SQLite may create `state.sqlite3-wal` and `state.sqlite3-shm`; SQLite owns those companion files and the application must not delete them manually. A bounded busy timeout applies to contention.

`PRAGMA user_version` is the sole authoritative schema version. There is no redundant migration-history table. The ordered v1 migration runs in an immediate transaction, creates only `storage_metadata`, inserts a storage-format marker, and updates `user_version` before commit. Every open reads the version, applies supported pending migrations, verifies the resulting version, and rejects a newer version without downgrade. Reopening v1 is idempotent.

## Failure and recovery

SQLite migration failure rolls back its transaction and returns a typed migration error. The database is never automatically deleted or recreated. Corrupt or non-SQLite bytes report `DatabaseCorrupt`; future schema versions report `UnsupportedDatabaseVersion`. Preserve the file and use a compatible build or restore a separately verified backup. P006 does not make backups. If a future backup operation is added for a live WAL database, it must use SQLite's backup API or another consistent snapshot mechanism; copying only the main database file is insufficient.

Permission errors are distinct from general filesystem errors. Config parsing errors do not include configuration contents. The app operates with the current user's permissions and does not require root. No credentials or telemetry are stored by P006, and no network operation is performed.

## Dependencies and portability

P006 pins Serde `1.0.229`, TOML `0.9.5`, rusqlite `0.32.1` with its bundled SQLite build, fs2 `0.4.3`, libc `0.2.191`, and thiserror `2.0.11`. Tests use tempfile `3.15.0`. Bundled SQLite avoids a system SQLite development dependency and improves reproducible CI builds, at the cost of compiling SQLite and carrying that library in the application dependency graph. All direct versions and the resolved lockfile are checked with Rust 1.85.0.

The persistence API does not include platform-specific paths in public service method signatures. Only Linux XDG resolution is implemented. macOS and FreeBSD must add their native application-directory policies before being described as supported.

## Test evidence

The storage contract tests use fresh temporary roots and cover default/load/save behavior, schema validation, XDG injection, file permissions, stale and concurrent writes, atomic pre-replacement failure, SQLite migration and metadata persistence, foreign-key configuration, rollback, future schemas, corrupt files, failed migrations, and bounded busy handling. They do not read or write real user configuration directories. The exact local and hosted validation outcomes are recorded in [TESTING.md](TESTING.md).

P022 runtime discovery is ephemeral. It does not create or modify SQLite or
TOML state, persist explicit roots, or attach runtime paths to support reports.

## P025 runtime installation

P025 provides qualified local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. Hosted Rust/GTK, GCC and Clang qualification passed in [CI run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). It does not establish publisher authenticity or support HTTPS acquisition. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for the workflow and limits.


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.
