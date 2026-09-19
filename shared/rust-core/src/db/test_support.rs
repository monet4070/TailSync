//! Isolated synthetic stores for tests/benchmarks. Never linked into production
//! unless the explicit dev-only test-support feature is selected.
use super::*;

impl HistoryDB {
    pub fn open_isolated_for_test(root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if root.exists() {
            return Err("isolated test store must not already exist".into());
        }
        crate::private_fs::create_private_dir_all(root)?;
        let conn = Connection::open(root.join("history-v2.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON; PRAGMA foreign_keys=ON;",
        )?;
        schema::initialize(&conn)?;
        let file_history_dir = root.join("file-history");
        let image_history_dir = root.join("image-history");
        crate::private_fs::create_private_dir_all(&file_history_dir)?;
        crate::private_fs::create_private_dir_all(&image_history_dir)?;
        Self::migrate(&conn, &file_history_dir, &image_history_dir)?;
        // In particular, never inspect or migrate the user's legacy v1 store.
        Ok(Self {
            conn,
            read_identity: std::sync::Arc::new(()),
            max_history: 100_000,
            storage_quota_bytes: crypto::DEFAULT_STORAGE_QUOTA_BYTES,
            storage_available: true,
            file_history_dir,
            image_history_dir,
        })
    }

    /// Fixed plaintext dataset, production encryption and schema. Seeding is
    /// outside measured intervals and deliberately bypasses UI retention caps.
    pub fn seed_synthetic_history_for_test(
        &mut self,
        rows: usize,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if rows > 50_000 {
            return Err("synthetic dataset is bounded at 50,000 rows".into());
        }
        let mut digest = blake3::Hasher::new();
        let transaction = self.conn.transaction()?;
        {
            let mut insert = transaction.prepare("INSERT INTO history(timestamp,type,description,data,size_bytes,source_peer,data_hash,category,categories,classifier_version) VALUES(?1,'text','',?2,?3,'fixture',?4,'text','[\"text\"]',1)")?;
            for index in 0..rows {
                let size = if index % 1000 == 0 { 64 * 1024 } else { 1024 };
                let mut text = format!(
                    "fixture-{index:08} {} ",
                    if index % 17 == 0 {
                        "needle"
                    } else {
                        "ordinary"
                    }
                );
                text.extend(std::iter::repeat_n(
                    char::from(b'a' + (index * 17 % 26) as u8),
                    size - text.len(),
                ));
                digest.update(text.as_bytes());
                let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
                let encrypted = crypto::encrypt(text.as_bytes())?;
                // Deliberate timestamp ties also exercise the id cursor.
                insert.execute(params![
                    format!("2026-09-08T00:00:{:02}Z", index % 60),
                    encrypted,
                    text.len() as i64,
                    hash
                ])?;
            }
        }
        transaction.commit()?;
        Ok(digest.finalize().to_hex().to_string())
    }
}
