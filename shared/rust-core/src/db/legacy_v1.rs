use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use fernet::Fernet;
use log::{info, warn};
use rusqlite::{Connection, OpenFlags};

use super::{get_data_dir, HistoryDB};

const REPORT_VERSION: u8 = 1;
const REPORT_FILE_NAME: &str = "v1-migration-report.json";

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct LegacyMigrationFailure {
    id: i64,
    error: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct LegacyMigrationReport {
    version: u8,
    source_size: u64,
    source_modified_nanos: u128,
    total_rows: usize,
    imported_rows: usize,
    failures: Vec<LegacyMigrationFailure>,
}

impl LegacyMigrationReport {
    fn matches(&self, source_size: u64, source_modified_nanos: u128) -> bool {
        self.version == REPORT_VERSION
            && self.source_size == source_size
            && self.source_modified_nanos == source_modified_nanos
    }
}

impl HistoryDB {
    pub(super) fn migrate_legacy_v1_if_present(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(legacy_directory) = legacy_data_directory() else {
            return Ok(());
        };
        self.migrate_legacy_v1_at(&legacy_directory, &get_data_dir().join(REPORT_FILE_NAME))
    }

    pub(super) fn migrate_legacy_v1_at(
        &mut self,
        legacy_directory: &Path,
        report_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let database_path = legacy_directory.join("history.db");
        if !database_path.is_file() {
            return Ok(());
        }
        let key_path = legacy_directory.join(".fernet_key");
        if !key_path.is_file() {
            return Err(format!(
                "legacy database exists but its Fernet key is missing: {}",
                key_path.display()
            )
            .into());
        }

        let metadata = database_path.metadata()?;
        let source_size = metadata.len();
        let source_modified_nanos = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
        if report_matches_source(report_path, source_size, source_modified_nanos) {
            return Ok(());
        }

        let key = std::fs::read_to_string(&key_path)?;
        let fernet = Fernet::new(key.trim()).ok_or("legacy Fernet key is invalid")?;
        let legacy = Connection::open_with_flags(&database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let rows = read_legacy_rows(&legacy)?;
        let total_rows = rows.len();
        let mut imported_rows = 0;
        let mut failures = Vec::new();

        info!("Automatically migrating {total_rows} TailSync v1 history entries...");
        for row in rows {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let token = std::str::from_utf8(&row.data)?;
                let plaintext = fernet.decrypt(token)?;
                let description = row.description.chars().take(100).collect::<String>();
                match row.entry_type.as_str() {
                    "text" => self.add_text_migrated(&row.timestamp, &description, &plaintext),
                    "image" => self.add_image_migrated(&row.timestamp, &description, &plaintext),
                    "file" => self.add_file_migrated(&row.timestamp, &description, &plaintext),
                    other => Err(format!("unsupported legacy history type: {other}").into()),
                }
            })();

            match result {
                Ok(()) => imported_rows += 1,
                Err(error) => failures.push(LegacyMigrationFailure {
                    id: row.id,
                    error: error.to_string().chars().take(500).collect(),
                }),
            }
        }

        let report = LegacyMigrationReport {
            version: REPORT_VERSION,
            source_size,
            source_modified_nanos,
            total_rows,
            imported_rows,
            failures,
        };
        write_report_atomic(report_path, &report)?;
        if report.failures.is_empty() {
            info!("TailSync v1 history migration completed: {imported_rows}/{total_rows}");
        } else {
            warn!(
                "TailSync v1 history migration completed with {} skipped entries; see {}",
                report.failures.len(),
                report_path.display()
            );
        }
        Ok(())
    }
}

struct LegacyRow {
    id: i64,
    timestamp: String,
    entry_type: String,
    description: String,
    data: Vec<u8>,
}

fn read_legacy_rows(connection: &Connection) -> Result<Vec<LegacyRow>, rusqlite::Error> {
    let mut statement =
        connection.prepare("SELECT id, time, type, desc, data FROM history ORDER BY id")?;
    let rows = statement
        .query_map([], |row| {
            let data = row.get_ref(4)?.as_bytes()?.to_vec();
            Ok(LegacyRow {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                entry_type: row.get(2)?,
                description: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                data,
            })
        })?
        .collect();
    rows
}

fn legacy_data_directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TAILSYNC_V1_DATA_DIR") {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.join("TailSync_History"))
}

fn report_matches_source(path: &Path, source_size: u64, source_modified_nanos: u128) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LegacyMigrationReport>(&bytes).ok())
        .is_some_and(|report| report.matches(source_size, source_modified_nanos))
}

fn write_report_atomic(
    path: &Path,
    report: &LegacyMigrationReport,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .ok_or("legacy migration report has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{REPORT_FILE_NAME}.{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::write(&temporary, serde_json::to_vec_pretty(report)?)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tailsync-{name}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_database(root: &Path) -> HistoryDB {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    type TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    data BLOB NOT NULL,
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    source_peer TEXT NOT NULL DEFAULT '',
                    data_hash TEXT NOT NULL DEFAULT '',
                    category TEXT NOT NULL DEFAULT 'text',
                    categories TEXT NOT NULL DEFAULT '[]',
                    category_confidence INTEGER NOT NULL DEFAULT 0,
                    classifier_version INTEGER NOT NULL DEFAULT 0,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    batch_id TEXT,
                    batch_index INTEGER,
                    batch_total INTEGER,
                    batch_status TEXT NOT NULL DEFAULT 'complete'
                );",
            )
            .unwrap();
        HistoryDB {
            conn: connection,
            max_history: 100,
            storage_quota_bytes: crate::crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: true,
            file_history_dir: root.join("file-history"),
            image_history_dir: root.join("image-history"),
        }
    }

    fn create_legacy_database(root: &Path) -> (Connection, Fernet) {
        let legacy_directory = root.join("TailSync_History");
        std::fs::create_dir_all(&legacy_directory).unwrap();
        let key = Fernet::generate_key();
        std::fs::write(legacy_directory.join(".fernet_key"), &key).unwrap();
        let fernet = Fernet::new(&key).unwrap();
        let connection = Connection::open(legacy_directory.join("history.db")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE history (
                    id INTEGER PRIMARY KEY,
                    time TEXT NOT NULL,
                    type TEXT NOT NULL,
                    desc TEXT,
                    data BLOB NOT NULL
                );",
            )
            .unwrap();
        (connection, fernet)
    }

    fn insert_legacy_row(
        connection: &Connection,
        fernet: &Fernet,
        id: i64,
        entry_type: &str,
        description: &str,
        plaintext: &[u8],
    ) {
        connection
            .execute(
                "INSERT INTO history (id, time, type, desc, data)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    id,
                    format!("2026-01-{id:02}T00:00:00Z"),
                    entry_type,
                    description,
                    fernet.encrypt(plaintext),
                ],
            )
            .unwrap();
    }

    #[test]
    fn legacy_migration_imports_all_types_and_is_idempotent() {
        let root = temporary_root("legacy-v1");
        let legacy_directory = root.join("TailSync_History");
        let report_path = root.join("data").join(REPORT_FILE_NAME);
        let (legacy, fernet) = create_legacy_database(&root);
        let text = b"legacy text";
        let image = {
            let mut bytes = Vec::from([2, 0, 0, 0, 1, 0, 0, 0]);
            bytes.extend_from_slice(&[0x7f; 8]);
            bytes
        };
        let file = b"legacy file bytes";
        insert_legacy_row(&legacy, &fernet, 1, "text", "note", text);
        insert_legacy_row(&legacy, &fernet, 2, "image", "picture", &image);
        insert_legacy_row(&legacy, &fernet, 3, "file", "../report.bin", file);
        drop(legacy);

        let mut database = test_database(&root);
        database
            .migrate_legacy_v1_at(&legacy_directory, &report_path)
            .unwrap();

        let entries = database.get_all(None, None, 10, 0).unwrap();
        assert_eq!(entries.len(), 3);
        for expected in [text.as_slice(), image.as_slice(), file.as_slice()] {
            let entry = entries
                .iter()
                .find(|entry| database.get_data(entry.id).unwrap() == expected)
                .expect("migrated payload should be readable");
            assert_eq!(entry.source_peer, "migrated");
        }
        let file_entry = entries
            .iter()
            .find(|entry| entry.entry_type == "file")
            .unwrap();
        let file_path = database.get_file_path(file_entry.id).unwrap().unwrap();
        assert!(file_path.starts_with(root.join("clipboard-files")));
        assert_eq!(file_path.file_name().unwrap(), "report.bin");
        assert_eq!(std::fs::read(file_path).unwrap(), file);

        database
            .migrate_legacy_v1_at(&legacy_directory, &report_path)
            .unwrap();
        assert_eq!(database.get_all(None, None, 10, 0).unwrap().len(), 3);
        assert!(legacy_directory.join("history.db").is_file());
        assert!(legacy_directory.join(".fernet_key").is_file());

        let report: LegacyMigrationReport =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(report.total_rows, 3);
        assert_eq!(report.imported_rows, 3);
        assert!(report.failures.is_empty());
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_migration_records_bad_rows_and_retries_after_source_changes() {
        let root = temporary_root("legacy-v1-retry");
        let legacy_directory = root.join("TailSync_History");
        let report_path = root.join("data").join(REPORT_FILE_NAME);
        let (legacy, fernet) = create_legacy_database(&root);
        insert_legacy_row(&legacy, &fernet, 1, "text", "good", b"first");
        legacy
            .execute(
                "INSERT INTO history (id, time, type, desc, data)
                 VALUES (2, '2026-01-02T00:00:00Z', 'text', 'bad', 'not-a-token')",
                [],
            )
            .unwrap();
        drop(legacy);

        let mut database = test_database(&root);
        database
            .migrate_legacy_v1_at(&legacy_directory, &report_path)
            .unwrap();
        assert_eq!(database.get_all(None, None, 10, 0).unwrap().len(), 1);
        let first_report: LegacyMigrationReport =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(first_report.imported_rows, 1);
        assert_eq!(first_report.failures.len(), 1);
        assert_eq!(first_report.failures[0].id, 2);

        let legacy = Connection::open(legacy_directory.join("history.db")).unwrap();
        insert_legacy_row(&legacy, &fernet, 3, "text", "later", b"second");
        drop(legacy);
        database
            .migrate_legacy_v1_at(&legacy_directory, &report_path)
            .unwrap();
        assert_eq!(database.get_all(None, None, 10, 0).unwrap().len(), 2);
        let second_report: LegacyMigrationReport =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(second_report.total_rows, 3);
        assert_eq!(second_report.failures.len(), 1);

        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}
