use rusqlite::{Connection, Error as SqliteError, Transaction, TransactionBehavior};

use crate::{DATABASE_SCHEMA_VERSION, StorageError};

pub(crate) fn apply_pending(
    connection: &mut Connection,
    path: &std::path::Path,
) -> Result<u32, StorageError> {
    apply_pending_with_hook(connection, path, |_| Ok(()))
}

fn apply_pending_with_hook<F>(
    connection: &mut Connection,
    path: &std::path::Path,
    mut after_migration: F,
) -> Result<u32, StorageError>
where
    F: FnMut(u32) -> rusqlite::Result<()>,
{
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| crate::database::classify_sqlite(path, source))?;
    let mut version = transaction
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(|source| crate::database::classify_sqlite(path, source))?;

    if version > DATABASE_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedDatabaseVersion {
            path: path.to_path_buf(),
            found: version,
            supported: DATABASE_SCHEMA_VERSION,
        });
    }

    while version < DATABASE_SCHEMA_VERSION {
        let target = version + 1;
        let result = match version {
            0 => migrate_v1(&transaction),
            _ => Err(SqliteError::InvalidQuery),
        };
        result.map_err(|source| crate::database::classify_migration(path, target, source))?;
        after_migration(target)
            .map_err(|source| crate::database::classify_migration(path, target, source))?;
        version = target;
    }

    transaction
        .commit()
        .map_err(|source| crate::database::classify_sqlite(path, source))?;
    Ok(version)
}

fn migrate_v1(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE storage_metadata (
             key TEXT PRIMARY KEY NOT NULL
                 CHECK(length(key) BETWEEN 1 AND 128)
                 CHECK(key NOT GLOB '*[^a-z0-9_.-]*'),
             value TEXT NOT NULL CHECK(length(value) <= 65536)
         ) WITHOUT ROWID;
         INSERT INTO storage_metadata(key, value)
             VALUES ('storage_format', 'endfield-unix');",
    )?;
    transaction.pragma_update(None, "user_version", 1_u32)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn failure_after_schema_creation_rolls_back_table_and_version() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("migration.sqlite3");
        let mut connection = Connection::open(&path).expect("empty database");

        let result =
            apply_pending_with_hook(&mut connection, &path, |_| Err(SqliteError::InvalidQuery));

        assert!(matches!(
            result,
            Err(StorageError::Migration { version: 1, .. })
        ));
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version remains zero");
        assert_eq!(version, 0);
        let table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='storage_metadata')",
                [],
                |row| row.get(0),
            )
            .expect("schema lookup");
        assert!(!table_exists, "new table is rolled back with the version");
    }
}
