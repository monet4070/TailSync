use fernet::Fernet;
use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tailsync_core::db::HistoryDB;

const WORKER_ENV: &str = "TAILSYNC_LEGACY_ISOLATION_WORKER";

#[test]
fn overridden_data_dir_does_not_import_implicit_legacy_history() {
    let audit_root = std::env::var_os("TAILSYNC_LEGACY_ISOLATION_AUDIT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = audit_root.join(format!(
        "legacy-isolation-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let home = root.join("synthetic-home");
    let legacy = home.join("TailSync_History");
    let isolated = root.join("isolated-data");
    fs::create_dir_all(&legacy).expect("create synthetic legacy directory");
    let key = Fernet::generate_key();
    fs::write(legacy.join(".fernet_key"), &key).expect("write synthetic legacy key");
    let fernet = Fernet::new(&key).expect("synthetic Fernet key");
    let legacy_db = Connection::open(legacy.join("history.db")).expect("legacy fixture database");
    legacy_db
        .execute_batch(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY,
                time TEXT NOT NULL,
                type TEXT NOT NULL,
                desc TEXT,
                data BLOB NOT NULL
            );",
        )
        .expect("legacy fixture schema");
    legacy_db
        .execute(
            "INSERT INTO history (id, time, type, desc, data)
             VALUES (1, '2026-01-01T00:00:00Z', 'text', 'synthetic', ?1)",
            params![fernet.encrypt(b"synthetic legacy content")],
        )
        .expect("legacy fixture entry");
    drop(legacy_db);

    let status = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "legacy_isolation_worker",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(WORKER_ENV, "1")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("TAILSYNC_DATA_DIR", &isolated)
        .env_remove("TAILSYNC_STORAGE_DIR")
        .env_remove("TAILSYNC_V1_DATA_DIR")
        .env("TAILSYNC_EXPECT_LEGACY_ROWS", "0")
        .status()
        .expect("start isolated legacy migration worker");
    assert!(
        status.success(),
        "isolated worker imported implicit legacy history"
    );

    if std::env::var_os("TAILSYNC_LEGACY_ISOLATION_AUDIT_DIR").is_none() {
        fs::remove_dir_all(root).expect("remove synthetic fixture");
    }
}

#[test]
#[ignore = "worker launched with synthetic HOME and isolated TAILSYNC_DATA_DIR"]
fn legacy_isolation_worker() {
    if std::env::var_os(WORKER_ENV).is_none() {
        return;
    }
    let database = HistoryDB::new().expect("open isolated database");
    let entries = database
        .get_all(None, None, 10, 0)
        .expect("read isolated history");
    let expected_rows: usize = std::env::var("TAILSYNC_EXPECT_LEGACY_ROWS")
        .expect("expected row count")
        .parse()
        .expect("numeric expected row count");
    assert_eq!(
        entries.len(),
        expected_rows,
        "legacy migration isolation policy"
    );
    assert!(
        !isolated_data_dir()
            .join("v1-migration-report.json")
            .exists(),
        "isolated worker attempted an implicit legacy migration"
    );
}

fn isolated_data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("TAILSYNC_DATA_DIR").expect("isolated data directory"))
}
