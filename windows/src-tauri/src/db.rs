use log::{info, warn};
use rusqlite::{params, params_from_iter, types::Value, Connection};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::crypto;
use crate::history_classifier::{self, Classification};

/// Get the application data directory
pub fn get_data_dir() -> PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR
        .get_or_init(|| {
            let dir = std::env::var_os("TAILSYNC_DATA_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    directories::ProjectDirs::from("com", "tailsync", "TailSync")
                        .map(|d| d.data_dir().to_path_buf())
                })
                .unwrap_or_else(|| {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join(".tailsync")
                });
            std::fs::create_dir_all(&dir).ok();
            dir
        })
        .clone()
}

/// Database schema version
const SCHEMA_VERSION: i64 = 6;
const FILE_REFERENCE_MAGIC: &[u8] = b"TSFILE1\0";
const IMAGE_REFERENCE_MAGIC: &[u8] = b"TSIMAGE1";
const MAX_STORED_ORIGINAL_NAME_BYTES: usize = 120;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredFileReference {
    version: u8,
    file_name: String,
}

pub fn get_file_history_dir() -> PathBuf {
    get_data_dir().join("file-history")
}

pub fn get_image_history_dir() -> PathBuf {
    get_data_dir().join("image-history")
}

pub fn get_incoming_dir() -> PathBuf {
    get_data_dir().join("incoming")
}

pub fn get_clipboard_files_dir() -> PathBuf {
    get_data_dir().join("clipboard-files")
}

fn encode_file_reference(file_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut encoded = FILE_REFERENCE_MAGIC.to_vec();
    encoded.extend_from_slice(&serde_json::to_vec(&StoredFileReference {
        version: 1,
        file_name: file_name.to_string(),
    })?);
    Ok(encoded)
}

fn encode_image_reference(file_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut encoded = IMAGE_REFERENCE_MAGIC.to_vec();
    encoded.extend_from_slice(&serde_json::to_vec(&StoredFileReference {
        version: 1,
        file_name: file_name.to_string(),
    })?);
    Ok(encoded)
}

fn decode_file_reference(data: &[u8]) -> Option<StoredFileReference> {
    let json = data.strip_prefix(FILE_REFERENCE_MAGIC)?;
    let reference = serde_json::from_slice::<StoredFileReference>(json).ok()?;
    (reference.version == 1).then_some(reference)
}

fn decode_image_reference(data: &[u8]) -> Option<StoredFileReference> {
    let json = data.strip_prefix(IMAGE_REFERENCE_MAGIC)?;
    let reference = serde_json::from_slice::<StoredFileReference>(json).ok()?;
    (reference.version == 1).then_some(reference)
}

fn resolve_file_reference_at(
    directory: &Path,
    reference: &StoredFileReference,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let relative = Path::new(&reference.file_name);
    if relative.components().count() != 1 || relative.file_name().is_none() {
        return Err("Invalid history file reference".into());
    }
    Ok(directory.join(relative))
}

fn sanitize_history_file_name(original_name: &str) -> String {
    let base_name = original_name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut sanitized = String::new();
    let mut bytes = 0;

    for character in base_name.chars() {
        let character = if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
            '_'
        } else {
            character
        };
        let width = character.len_utf8();
        if bytes + width > MAX_STORED_ORIGINAL_NAME_BYTES {
            break;
        }
        sanitized.push(character);
        bytes += width;
    }

    let sanitized = sanitized.trim_matches(|character| character == ' ' || character == '.');
    if sanitized.is_empty() {
        "file".to_string()
    } else {
        sanitized.to_string()
    }
}

fn persist_history_file_at(
    directory: &Path,
    data_hash: &str,
    original_name: &str,
    data: &[u8],
) -> Result<(Vec<u8>, PathBuf), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let safe_name = sanitize_history_file_name(original_name);
    let file_name = format!("{data_hash}-{safe_name}");
    let target = directory.join(&file_name);

    let target_matches = target
        .metadata()
        .map(|metadata| metadata.len() == data.len() as u64)
        .unwrap_or(false);
    if !target_matches {
        let temporary = directory.join(format!("{file_name}.{}.tmp", std::process::id()));
        std::fs::write(&temporary, data)?;
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(&temporary, &target)?;
    }

    Ok((encode_file_reference(&file_name)?, target))
}

fn persist_image_at(
    directory: &Path,
    data_hash: &str,
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let file_name = format!("{data_hash}.bin");
    let target = directory.join(&file_name);
    if !target.is_file() {
        let encrypted = crypto::encrypt(data)?;
        let temporary = directory.join(format!("{file_name}.{}.tmp", std::process::id()));
        std::fs::write(&temporary, encrypted)?;
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(temporary, target)?;
    }
    encode_image_reference(&file_name)
}

fn persist_history_file_from_path_at(
    directory: &Path,
    data_hash: &str,
    original_name: &str,
    source: &Path,
    size: u64,
    move_source: bool,
) -> Result<(Vec<u8>, PathBuf), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let safe_name = sanitize_history_file_name(original_name);
    let file_name = format!("{data_hash}-{safe_name}");
    let target = directory.join(&file_name);

    let target_matches = target
        .metadata()
        .map(|metadata| metadata.len() == size)
        .unwrap_or(false);
    if !target_matches {
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        if move_source {
            match std::fs::rename(source, &target) {
                Ok(()) => {}
                Err(_) => {
                    std::fs::copy(source, &target)?;
                    std::fs::remove_file(source)?;
                }
            }
        } else {
            let temporary = directory.join(format!("{file_name}.{}.tmp", std::process::id()));
            std::fs::copy(source, &temporary)?;
            std::fs::rename(temporary, &target)?;
        }
    } else if move_source && source != target {
        let _ = std::fs::remove_file(source);
    }

    Ok((encode_file_reference(&file_name)?, target))
}

fn materialize_clipboard_file_at(
    directory: &Path,
    source: &Path,
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !source.is_file() {
        return Err(format!("Clipboard source file is missing: {}", source.display()).into());
    }
    let safe_name = sanitize_history_file_name(original_name);
    let transfer_directory = directory.join(format!("{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&transfer_directory)?;
    let target = transfer_directory.join(safe_name);
    if std::fs::hard_link(source, &target).is_err() {
        std::fs::copy(source, &target)?;
    }
    Ok(target)
}

pub fn materialize_clipboard_file(
    source: &Path,
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    materialize_clipboard_file_at(&get_clipboard_files_dir(), source, original_name)
}

pub struct HistoryDB {
    conn: Connection,
    max_history: i64,
    file_history_dir: PathBuf,
    image_history_dir: PathBuf,
}

/// A clipboard history entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String, // ISO 8601
    #[serde(rename = "type")]
    pub entry_type: String, // "text" | "image" | "file"
    pub description: String,
    pub data_hash: String,
    pub size_bytes: i64,
    pub source_peer: String,
    pub category: String,
    pub categories: Vec<String>,
    pub category_confidence: i64,
    pub classifier_version: i64,
}

impl HistoryDB {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = get_data_dir().join("history-v2.db");
        info!("Opening database at {}", db_path.display());

        let conn = Connection::open(&db_path)?;
        let file_history_dir = get_file_history_dir();
        let image_history_dir = get_image_history_dir();

        // Enable WAL mode for concurrent read/write
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT NOT NULL,
                type        TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data        BLOB NOT NULL,
                size_bytes  INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash   TEXT NOT NULL DEFAULT '',
                category    TEXT NOT NULL DEFAULT 'text',
                categories  TEXT NOT NULL DEFAULT '[]',
                category_confidence INTEGER NOT NULL DEFAULT 0,
                classifier_version INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_history_timestamp
                ON history(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_history_type
                ON history(type);
            CREATE INDEX IF NOT EXISTS idx_history_description
                ON history(description);
            CREATE INDEX IF NOT EXISTS idx_history_hash
                ON history(data_hash);",
        )?;

        // Run migrations
        Self::migrate(&conn, &file_history_dir, &image_history_dir)?;

        // Transfers cannot survive a process restart, so stale incoming files
        // are safe to remove before the network server starts.
        let incoming = get_incoming_dir();
        if let Err(error) = std::fs::remove_dir_all(&incoming) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not clean incoming transfer folder: {error}");
            }
        }
        std::fs::create_dir_all(&incoming)?;
        let clipboard_files = get_clipboard_files_dir();
        if let Err(error) = std::fs::remove_dir_all(&clipboard_files) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not clean clipboard files folder: {error}");
            }
        }
        std::fs::create_dir_all(&clipboard_files)?;
        std::fs::create_dir_all(&file_history_dir)?;
        std::fs::create_dir_all(&image_history_dir)?;

        Ok(HistoryDB {
            conn,
            max_history: 1000,
            file_history_dir,
            image_history_dir,
        })
    }

    /// Update history limit from settings.
    pub fn set_max_history(&mut self, limit: i64) {
        self.max_history = limit;
    }

    fn migrate(
        conn: &Connection,
        file_history_dir: &Path,
        image_history_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version < 1 {
            info!("Running database migration v1...");
            // Base schema already created above
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }

        if version < 2 {
            info!("Running database migration v2...");
            // Future: add full-text search, etc.
            conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])?;
        }

        if version < 3 {
            info!("Running database migration v3 (files to local history folder)...");
            let legacy_files = {
                let mut statement = conn.prepare(
                    "SELECT id, data, data_hash, description FROM history WHERE type = 'file'",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };

            for (id, stored, stored_hash, description) in legacy_files {
                if decode_file_reference(&stored).is_some() {
                    continue;
                }
                match crypto::decrypt(&stored) {
                    Ok(data) => {
                        let actual_hash = blake3::hash(&data).to_hex().to_string();
                        if !stored_hash.is_empty() && stored_hash != actual_hash {
                            warn!("File history hash corrected during migration for entry {id}");
                        }
                        let (reference, _) = persist_history_file_at(
                            file_history_dir,
                            &actual_hash,
                            &description,
                            &data,
                        )?;
                        conn.execute(
                            "UPDATE history SET data = ?1, data_hash = ?2, size_bytes = ?3 WHERE id = ?4",
                            params![reference, actual_hash, data.len() as i64, id],
                        )?;
                    }
                    Err(error) => warn!("Legacy file entry {id} could not be migrated: {error}"),
                }
            }
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![3_i64],
            )?;
        }

        if version < 4 {
            info!("Running database migration v4 (images to local history folder)...");
            let legacy_images = {
                let mut statement =
                    conn.prepare("SELECT id, data, data_hash FROM history WHERE type = 'image'")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (id, stored, stored_hash) in legacy_images {
                if decode_image_reference(&stored).is_some() {
                    continue;
                }
                match crypto::decrypt(&stored) {
                    Ok(data) => {
                        let actual_hash = blake3::hash(&data).to_hex().to_string();
                        if !stored_hash.is_empty() && stored_hash != actual_hash {
                            warn!("Image history hash corrected during migration for entry {id}");
                        }
                        let reference = persist_image_at(image_history_dir, &actual_hash, &data)?;
                        conn.execute(
                            "UPDATE history SET data = ?1, data_hash = ?2, size_bytes = ?3 WHERE id = ?4",
                            params![reference, actual_hash, data.len() as i64, id],
                        )?;
                    }
                    Err(error) => warn!("Legacy image entry {id} could not be migrated: {error}"),
                }
            }
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![4_i64],
            )?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
            if page_count > 0 && free_pages * 4 > page_count {
                info!("Reclaiming SQLite pages after image migration...");
                conn.execute_batch("VACUUM;")?;
            }
        }

        if version < 5 {
            info!("Running database migration v5 (history categories)...");
        }
        Self::add_column_if_missing(
            conn,
            "category",
            "ALTER TABLE history ADD COLUMN category TEXT NOT NULL DEFAULT 'text'",
        )?;
        Self::add_column_if_missing(
            conn,
            "category_confidence",
            "ALTER TABLE history ADD COLUMN category_confidence INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "classifier_version",
            "ALTER TABLE history ADD COLUMN classifier_version INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_history_category_timestamp
             ON history(category, timestamp DESC, id DESC);",
        )?;
        if version < 5 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![5_i64],
            )?;
        }

        if version < 6 {
            info!("Running database migration v6 (multiple history labels)...");
        }
        let categories_added = Self::add_column_if_missing(
            conn,
            "categories",
            "ALTER TABLE history ADD COLUMN categories TEXT NOT NULL DEFAULT '[\"text\"]'",
        )?;
        if version < 6 || categories_added {
            conn.execute("UPDATE history SET categories = json_array(category)", [])?;
        }
        if version < 6 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }
        conn.execute(
            "UPDATE history
             SET category = type,
                 categories = json_array(type),
                 category_confidence = 100,
                 classifier_version = ?1
             WHERE type IN ('image', 'file')
               AND (classifier_version < ?1 OR category != type OR categories != json_array(type))",
            params![history_classifier::CLASSIFIER_VERSION],
        )?;

        Ok(())
    }

    fn add_column_if_missing(
        conn: &Connection,
        column: &str,
        sql: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let columns = conn
            .prepare("PRAGMA table_info(history)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if !columns.iter().any(|existing| existing == column) {
            conn.execute(sql, [])?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Add a text entry to history. Duplicate: delete old, insert new at top.
    pub fn add_text(
        &mut self,
        text: &str,
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        // Delete old entry with same hash so the new one is at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let encrypted = crypto::encrypt(text.as_bytes())?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let description = text.chars().take(100).collect::<String>();
        let classification = history_classifier::classify_text(text);
        let categories = serde_json::to_string(&classification.categories())?;

        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                timestamp,
                description,
                encrypted,
                text.len() as i64,
                source_peer,
                data_hash,
                classification.category,
                categories,
                classification.confidence as i64,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;

        self.trim("text")?;
        Ok(())
    }

    /// Add an image entry to history. Duplicate: delete old, insert new.
    pub fn add_image(
        &mut self,
        image_data: &[u8],
        source_peer: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(image_data).to_hex().to_string();
        // Delete old entry so the new copy appears at the top
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries(&duplicate_ids)?;

        let reference = persist_image_at(&self.image_history_dir, &data_hash, image_data)?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let description = format!("Image {} bytes", image_data.len());

        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, ?5, ?6, 'image', '[\"image\"]', 100, ?7)",
            params![
                timestamp,
                description,
                reference,
                image_data.len() as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;

        self.trim("image")?;
        Ok(())
    }

    /// Check if an entry with the given hash already exists
    fn exists_by_hash(&self, hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE data_hash = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get history entries with optional keyword search
    pub fn get_all(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        self.get_all_filtered(keyword, category, None, None, limit, offset)
    }

    /// Query history by keyword, any assigned label, and an optional UTC time range.
    /// The start is inclusive and the end is exclusive.
    pub fn get_all_filtered(
        &self,
        keyword: Option<&str>,
        category: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let keyword = keyword.filter(|value| !value.trim().is_empty());
        let category = category.filter(|value| !value.is_empty() && *value != "all");
        if category.is_some_and(|value| !history_classifier::is_known_category(value)) {
            return Err(format!("Unsupported history category: {}", category.unwrap()).into());
        }

        let parse_bound = |value: Option<&str>, name: &str| {
            value
                .map(|value| {
                    chrono::DateTime::parse_from_rfc3339(value)
                        .map(|date| date.with_timezone(&chrono::Utc))
                        .map_err(|_| format!("Invalid {name}: expected an RFC 3339 timestamp"))
                })
                .transpose()
        };
        let start_time = parse_bound(start_time, "start_time")?;
        let end_time = parse_bound(end_time, "end_time")?;
        if start_time
            .as_ref()
            .zip(end_time.as_ref())
            .is_some_and(|(start, end)| start >= end)
        {
            return Err("start_time must be earlier than end_time".into());
        }

        let mut conditions = Vec::new();
        let mut values = Vec::<Value>::new();
        if let Some(keyword) = keyword {
            let pattern = format!("%{keyword}%");
            conditions.push(
                "(description LIKE ? OR source_peer LIKE ? OR type LIKE ? OR category LIKE ?
                  OR EXISTS (SELECT 1 FROM json_each(history.categories) AS label
                             WHERE label.value LIKE ?))",
            );
            for _ in 0..5 {
                values.push(Value::Text(pattern.clone()));
            }
        }
        if let Some(category) = category {
            conditions.push(
                "EXISTS (SELECT 1 FROM json_each(history.categories) AS label
                         WHERE label.value = ?)",
            );
            values.push(Value::Text(category.to_string()));
        }
        if let Some(start_time) = start_time {
            conditions.push("julianday(timestamp) >= julianday(?)");
            values.push(Value::Text(start_time.to_rfc3339()));
        }
        if let Some(end_time) = end_time {
            conditions.push("julianday(timestamp) < julianday(?)");
            values.push(Value::Text(end_time.to_rfc3339()));
        }

        let mut sql = String::from(
            "SELECT id, timestamp, type, description, data_hash, size_bytes, source_peer,
                    category, category_confidence, classifier_version, categories
             FROM history",
        );
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(limit as i64));
        values.push(Value::Integer(offset as i64));

        let entries = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(values.iter()), Self::row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Get entry bytes. File entries backed by the history folder are read
    /// from disk; legacy file blobs remain readable through decryption.
    pub fn get_data(&self, id: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let (entry_type, stored): (String, Vec<u8>) = self.conn.query_row(
            "SELECT type, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if entry_type == "text" {
            return self.read_text_data(&stored);
        }
        if entry_type == "file" {
            if let Some(reference) = decode_file_reference(&stored) {
                return Ok(std::fs::read(resolve_file_reference_at(
                    &self.file_history_dir,
                    &reference,
                )?)?);
            }
        }
        if entry_type == "image" {
            if let Some(reference) = decode_image_reference(&stored) {
                let encrypted = std::fs::read(resolve_file_reference_at(
                    &self.image_history_dir,
                    &reference,
                )?)?;
                return crypto::decrypt(&encrypted);
            }
        }
        crypto::decrypt(&stored)
    }

    fn read_text_data(&self, stored: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if let Some(reference) = decode_image_reference(stored) {
            let encrypted = std::fs::read(resolve_file_reference_at(
                &self.image_history_dir,
                &reference,
            )?)?;
            return crypto::decrypt(&encrypted);
        }
        crypto::decrypt(stored)
    }

    pub fn backfill_classifications(
        &mut self,
        limit: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let media_updated = self.conn.execute(
            "UPDATE history
             SET category = type,
                 categories = json_array(type),
                 category_confidence = 100,
                 classifier_version = ?1
             WHERE type IN ('image', 'file')
               AND (classifier_version < ?1 OR category != type
                    OR categories != json_array(type) OR category_confidence != 100)",
            params![history_classifier::CLASSIFIER_VERSION],
        )?;
        let rows = self
            .conn
            .prepare(
                "SELECT id, data FROM history
                 WHERE type = 'text' AND classifier_version < ?1
                 ORDER BY id ASC LIMIT ?2",
            )?
            .query_map(
                params![history_classifier::CLASSIFIER_VERSION, limit as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(media_updated);
        }

        let mut updates = Vec::with_capacity(rows.len());
        for (id, stored) in rows {
            let legacy_reference = decode_image_reference(&stored);
            let (classification, replacement) = match self.read_text_data(&stored) {
                Ok(data) => match std::str::from_utf8(&data) {
                    Ok(text) => (
                        history_classifier::classify_text(text),
                        if legacy_reference.is_some() {
                            Some(crypto::encrypt(&data)?)
                        } else {
                            None
                        },
                    ),
                    Err(error) => {
                        warn!("History text entry {id} is not UTF-8: {error}");
                        (
                            Classification {
                                category: "text",
                                confidence: 0,
                                secondary_category: None,
                            },
                            None,
                        )
                    }
                },
                Err(error) => {
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io_error| io_error.kind() != std::io::ErrorKind::NotFound)
                    {
                        return Err(error);
                    }
                    warn!("History text entry {id} could not be classified: {error}");
                    (
                        Classification {
                            category: "text",
                            confidence: 0,
                            secondary_category: None,
                        },
                        None,
                    )
                }
            };
            updates.push((id, classification, replacement, legacy_reference));
        }

        let tx = self.conn.transaction()?;
        for (id, classification, replacement, _) in &updates {
            let categories = serde_json::to_string(&classification.categories())?;
            if let Some(encrypted) = replacement {
                tx.execute(
                    "UPDATE history
                     SET data = ?1, category = ?2, categories = ?3,
                         category_confidence = ?4, classifier_version = ?5
                     WHERE id = ?6",
                    params![
                        encrypted,
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            } else {
                tx.execute(
                    "UPDATE history
                     SET category = ?1, categories = ?2,
                         category_confidence = ?3, classifier_version = ?4
                     WHERE id = ?5",
                    params![
                        classification.category,
                        categories,
                        classification.confidence as i64,
                        history_classifier::CLASSIFIER_VERSION,
                        id,
                    ],
                )?;
            }
        }
        tx.commit()?;

        for (_, _, replacement, reference) in &updates {
            if replacement.is_none() {
                continue;
            }
            if let Some(reference) = reference {
                let path = resolve_file_reference_at(&self.image_history_dir, reference)?;
                let remaining: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM history WHERE data = ?1",
                    params![encode_image_reference(&reference.file_name)?],
                    |row| row.get(0),
                )?;
                if remaining == 0 {
                    if let Err(error) = std::fs::remove_file(&path) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            warn!(
                                "Could not remove migrated text reference {}: {error}",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
        Ok(media_updated + updates.len())
    }

    /// Return the on-disk path for a folder-backed file history entry.
    /// Legacy BLOB entries return None and continue through get_data().
    pub fn get_file_path(&self, id: i64) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let (entry_type, stored): (String, Vec<u8>) = self.conn.query_row(
            "SELECT type, data FROM history WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if entry_type != "file" {
            return Ok(None);
        }
        let Some(reference) = decode_file_reference(&stored) else {
            return Ok(None);
        };
        let path = resolve_file_reference_at(&self.file_history_dir, &reference)?;
        if !path.is_file() {
            return Err(format!("History file is missing: {}", path.display()).into());
        }
        Ok(Some(path))
    }

    /// Get the entry description for an entry (filename for file entries).
    pub fn get_description(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let desc: String = self.conn.query_row(
            "SELECT description FROM history WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(desc)
    }

    /// Get the entry type ("text" | "image" | "file") for an entry.
    pub fn get_type(&self, id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let t: String = self.conn.query_row(
            "SELECT type FROM history WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(t)
    }

    /// Delete a history entry
    pub fn delete(&mut self, id: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.delete_entries(&[id])
    }

    /// Remove every history entry in one transaction.
    pub fn clear_all(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM history", [])?;
        tx.commit()?;
        if let Err(error) = std::fs::remove_dir_all(&self.file_history_dir) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not remove file history folder: {error}");
            }
        }
        std::fs::create_dir_all(&self.file_history_dir)?;
        if let Err(error) = std::fs::remove_dir_all(&self.image_history_dir) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!("Could not remove image history folder: {error}");
            }
        }
        std::fs::create_dir_all(&self.image_history_dir)?;
        // Reclaim pages after an explicit user-initiated clear operation.
        let _ = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        Ok(())
    }

    /// Trim entries of a given type beyond the configured limit
    fn trim(&mut self, entry_type: &str) -> Result<(), Box<dyn std::error::Error>> {
        let max = match entry_type {
            "text" => self.max_history,
            "image" | "file" => (self.max_history / 10).max(10),
            _ => 100,
        };

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE type = ?1",
            params![entry_type],
            |row| row.get(0),
        )?;

        if count > max {
            let excess = count - max;
            let ids = self.oldest_entry_ids(Some(entry_type), excess)?;
            self.delete_entries(&ids)?;
            info!("Trimmed {} {} entries", excess, entry_type);
        }

        // Also enforce total cap of 2000
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))?;
        let total_max = self.max_history;
        if total > total_max {
            let excess = total - total_max;
            let ids = self.oldest_entry_ids(None, excess)?;
            self.delete_entries(&ids)?;
            info!("Trimmed {} entries (total cap)", excess);
        }

        Ok(())
    }

    /// Add a file entry to history
    pub fn add_file(
        &mut self,
        name: &str,
        data: &[u8],
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        let (reference, file_path) =
            persist_history_file_at(&self.file_history_dir, &data_hash, name, data)?;
        let duplicate_ids = self.entry_ids_by_hash(&data_hash)?;
        self.delete_entries_except(&duplicate_ids, Some(&file_path))?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, ?5, ?6, 'file', '[\"file\"]', 100, ?7)",
            params![
                timestamp,
                name,
                reference,
                data.len() as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(file_path)
    }

    pub fn add_file_from_path(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.add_file_path_inner(name, source, data_hash, size, source_peer, false)
    }

    pub fn adopt_file(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.add_file_path_inner(name, source, data_hash, size, source_peer, true)
    }

    fn add_file_path_inner(
        &mut self,
        name: &str,
        source: &Path,
        data_hash: &str,
        size: u64,
        source_peer: &str,
        move_source: bool,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let (reference, file_path) = persist_history_file_from_path_at(
            &self.file_history_dir,
            data_hash,
            name,
            source,
            size,
            move_source,
        )?;
        let duplicate_ids = self.entry_ids_by_hash(data_hash)?;
        self.delete_entries_except(&duplicate_ids, Some(&file_path))?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, ?5, ?6, 'file', '[\"file\"]', 100, ?7)",
            params![
                timestamp,
                name,
                reference,
                size as i64,
                source_peer,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        self.trim("file")?;
        Ok(file_path)
    }

    // ── Migration inserts (pre-set timestamp + description) ──────

    pub fn add_text_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        let classification = std::str::from_utf8(data)
            .map(history_classifier::classify_text)
            .unwrap_or(Classification {
                category: "text",
                confidence: 0,
                secondary_category: None,
            });
        let categories = serde_json::to_string(&classification.categories())?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'text', ?2, ?3, ?4, 'migrated', ?5, ?6, ?7, ?8, ?9)",
            params![
                time,
                desc,
                encrypted,
                data.len() as i64,
                data_hash,
                classification.category,
                categories,
                classification.confidence as i64,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        Ok(())
    }

    pub fn add_image_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let encrypted = crypto::encrypt(data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'image', ?2, ?3, ?4, 'migrated', ?5, 'image', '[\"image\"]', 100, ?6)",
            params![
                time,
                desc,
                encrypted,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        Ok(())
    }

    pub fn add_file_migrated(
        &mut self,
        time: &str,
        desc: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash = blake3::hash(data).to_hex().to_string();
        if self.exists_by_hash(&data_hash)? {
            return Ok(());
        }
        let (reference, _) =
            persist_history_file_at(&self.file_history_dir, &data_hash, desc, data)?;
        self.conn.execute(
            "INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, categories, category_confidence, classifier_version)
             VALUES (?1, 'file', ?2, ?3, ?4, 'migrated', ?5, 'file', '[\"file\"]', 100, ?6)",
            params![
                time,
                desc,
                reference,
                data.len() as i64,
                data_hash,
                history_classifier::CLASSIFIER_VERSION,
            ],
        )?;
        Ok(())
    }

    fn oldest_entry_ids(
        &self,
        entry_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let ids = if let Some(entry_type) = entry_type {
            self.conn
                .prepare("SELECT id FROM history WHERE type = ?1 ORDER BY timestamp ASC LIMIT ?2")?
                .query_map(params![entry_type, limit], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            self.conn
                .prepare("SELECT id FROM history ORDER BY timestamp ASC LIMIT ?1")?
                .query_map(params![limit], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(ids)
    }

    fn entry_ids_by_hash(&self, data_hash: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let ids = self
            .conn
            .prepare("SELECT id FROM history WHERE data_hash = ?1")?
            .query_map(params![data_hash], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    fn delete_entries(&mut self, ids: &[i64]) -> Result<(), Box<dyn std::error::Error>> {
        self.delete_entries_except(ids, None)
    }

    fn delete_entries_except(
        &mut self,
        ids: &[i64],
        preserve_path: Option<&Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut references = Vec::new();
        for id in ids {
            let stored = self.conn.query_row(
                "SELECT type, data FROM history WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            );
            if let Ok((entry_type, stored)) = stored {
                let decoded = match entry_type.as_str() {
                    "file" => decode_file_reference(&stored)
                        .map(|reference| (self.file_history_dir.clone(), reference)),
                    "image" | "text" => decode_image_reference(&stored)
                        .map(|reference| (self.image_history_dir.clone(), reference)),
                    _ => None,
                };
                if let Some((directory, reference)) = decoded {
                    references.push((stored, directory, reference));
                }
            }
        }

        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        tx.commit()?;

        for (stored, directory, reference) in references {
            let remaining: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM history WHERE data = ?1",
                params![stored],
                |row| row.get(0),
            )?;
            if remaining == 0 {
                let path = resolve_file_reference_at(&directory, &reference)?;
                if preserve_path == Some(path.as_path()) {
                    continue;
                }
                if let Err(error) = std::fs::remove_file(&path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        warn!("Could not remove history file {}: {error}", path.display());
                    }
                }
            }
        }
        Ok(())
    }

    fn row_to_entry(row: &rusqlite::Row) -> Result<HistoryEntry, rusqlite::Error> {
        let category = row.get::<_, String>(7)?;
        let encoded_categories = row.get::<_, String>(10)?;
        let mut categories = serde_json::from_str::<Vec<String>>(&encoded_categories)
            .unwrap_or_else(|_| vec![category.clone()]);
        categories.retain(|label| history_classifier::is_known_category(label));
        categories.dedup();
        if !categories.iter().any(|label| label == &category) {
            categories.insert(0, category.clone());
        }
        Ok(HistoryEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            entry_type: row.get(2)?,
            description: row.get(3)?,
            data_hash: row.get(4)?,
            size_bytes: row.get(5)?,
            source_peer: row.get(6)?,
            category,
            categories,
            category_confidence: row.get(8)?,
            classifier_version: row.get(9)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database(root: &Path) -> HistoryDB {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
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
                classifier_version INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        HistoryDB {
            conn,
            max_history: 100,
            file_history_dir: root.join("file-history"),
            image_history_dir: root.join("image-history"),
        }
    }

    #[test]
    fn file_reference_round_trip_uses_external_file() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-db-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let data = b"large file history content";
        let hash = blake3::hash(data).to_hex().to_string();

        let (encoded, path) =
            persist_history_file_at(&directory, &hash, "archive.bin", data).unwrap();
        let reference = decode_file_reference(&encoded).unwrap();

        assert_eq!(reference.version, 1);
        assert_eq!(reference.file_name, format!("{hash}-archive.bin"));
        assert_eq!(path, directory.join(reference.file_name));
        assert_eq!(std::fs::read(&path).unwrap(), data);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_reference_rejects_parent_paths() {
        let directory = std::env::temp_dir();
        let reference = StoredFileReference {
            version: 1,
            file_name: "../outside.bin".to_string(),
        };
        assert!(resolve_file_reference_at(&directory, &reference).is_err());
    }

    #[test]
    fn file_reference_sanitizes_untrusted_original_name() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-file-name-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let data = b"file content";
        let hash = blake3::hash(data).to_hex().to_string();

        let (encoded, path) = persist_history_file_at(
            &directory,
            &hash,
            r#"C:\temp\bad<name>:"file"?.txt. "#,
            data,
        )
        .unwrap();
        let reference = decode_file_reference(&encoded).unwrap();

        assert_eq!(reference.file_name, format!("{hash}-bad_name___file__.txt"));
        assert_eq!(path.file_name().unwrap(), reference.file_name.as_str());
        assert_eq!(std::fs::read(&path).unwrap(), data);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clipboard_materialization_preserves_the_original_basename() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-clipboard-name-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let history_directory = directory.join("file-history");
        let clipboard_directory = directory.join("clipboard-files");
        let data = b"clipboard file content";
        let hash = blake3::hash(data).to_hex().to_string();
        let (_, history_path) =
            persist_history_file_at(&history_directory, &hash, "report.pdf", data).unwrap();

        let clipboard_path =
            materialize_clipboard_file_at(&clipboard_directory, &history_path, "report.pdf")
                .unwrap();

        assert_eq!(clipboard_path.file_name().unwrap(), "report.pdf");
        assert_eq!(std::fs::read(clipboard_path).unwrap(), data);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_history_lifecycle_keeps_bytes_out_of_sqlite() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-history-lifecycle-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = vec![0x5a; 1024 * 1024];

        let first_path = db.add_file("archive.bin", &data, "self").unwrap();
        assert!(first_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-archive.bin"));
        let first_id: i64 = db
            .conn
            .query_row("SELECT id FROM history", [], |row| row.get(0))
            .unwrap();
        let stored_size: i64 = db
            .conn
            .query_row("SELECT length(data) FROM history", [], |row| row.get(0))
            .unwrap();

        assert!(
            stored_size < 256,
            "SQLite should only contain a file reference"
        );
        assert_eq!(db.get_data(first_id).unwrap(), data);
        assert_eq!(
            db.get_file_path(first_id).unwrap(),
            Some(first_path.clone())
        );

        db.delete(first_id).unwrap();
        assert!(!first_path.exists());

        db.add_file("one.bin", b"one", "self").unwrap();
        db.add_file("two.bin", b"two", "self").unwrap();
        db.clear_all().unwrap();
        assert_eq!(std::fs::read_dir(&db.file_history_dir).unwrap().count(), 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn image_history_lifecycle_keeps_bytes_out_of_sqlite() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-image-history-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = vec![0x7b; 1024 * 1024];

        db.add_image(&data, "self").unwrap();
        let (id, stored_size): (i64, i64) = db
            .conn
            .query_row("SELECT id, length(data) FROM history", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        let image_path = std::fs::read_dir(&db.image_history_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert!(
            stored_size < 256,
            "SQLite should only contain an image reference"
        );
        assert_eq!(db.get_data(id).unwrap(), data);
        db.delete(id).unwrap();
        assert!(!image_path.exists());

        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_file_replacement_cleans_old_name_and_preserves_same_path() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-history-duplicate-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let data = b"same content";

        let old_path = db.add_file("old.bin", data, "self").unwrap();
        let new_path = db.add_file("new.bin", data, "self").unwrap();
        assert_ne!(old_path, new_path);
        assert!(!old_path.exists());
        assert!(new_path.exists());

        let same_path = db.add_file("new.bin", data, "self").unwrap();
        assert_eq!(same_path, new_path);
        assert!(same_path.exists());
        assert_eq!(std::fs::read(&same_path).unwrap(), data);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v5_is_idempotent_and_classifies_media() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (4);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'image', 'image', X'00', 1, 'self', 'hash');",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        let media: (String, i64, i64) = conn
            .query_row(
                "SELECT category, category_confidence, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(history)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(
            media,
            (
                "image".to_string(),
                100,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );
        assert!(columns.contains(&"category".to_string()));
        assert!(columns.contains(&"category_confidence".to_string()));
        assert!(columns.contains(&"classifier_version".to_string()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v5_repairs_a_partial_schema() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-partial-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (5);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash)
             VALUES ('2026-01-01T00:00:00Z', 'file', 'file', X'00', 1, 'self', 'hash');",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let media: (String, i64, i64) = conn
            .query_row(
                "SELECT category, category_confidence, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_history_category_timestamp'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            media,
            (
                "file".to_string(),
                100,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );
        assert_eq!(index_count, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_v6_preserves_v5_primary_categories_as_json_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-v6-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let file_dir = root.join("file-history");
        let image_dir = root.join("image-history");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::create_dir_all(&image_dir).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (5);
             CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                type TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                data BLOB NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                source_peer TEXT NOT NULL DEFAULT '',
                data_hash TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'text',
                category_confidence INTEGER NOT NULL DEFAULT 0,
                classifier_version INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO history
                (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                 category, category_confidence, classifier_version)
             VALUES ('2026-01-01T00:00:00Z', 'text', 'legacy code', X'00', 1,
                     'self', 'legacy-code', 'code', 90, 2);",
        )
        .unwrap();

        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();
        HistoryDB::migrate(&conn, &file_dir, &image_dir).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        let migrated: (String, String, i64) = conn
            .query_row(
                "SELECT category, categories, classifier_version FROM history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let categories: Vec<String> = serde_json::from_str(&migrated.1).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(history)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(version, 6);
        assert_eq!(migrated.0, "code");
        assert_eq!(categories, vec!["code"]);
        assert_eq!(migrated.2, 2);
        assert!(columns.contains(&"categories".to_string()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backfill_upgrades_v2_to_v3_and_persists_secondary_labels() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-v3-backfill-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut db = test_database(&root);
        let text = b"example.com";
        let encrypted = crypto::encrypt(text).unwrap();
        let hash = blake3::hash(text).to_hex().to_string();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, categories, category_confidence, classifier_version)
                 VALUES ('2026-02-01T10:00:00Z', 'text', 'example.com', ?1, ?2,
                         'self', ?3, 'text', '[\"text\"]', 75, 2)",
                params![encrypted, text.len() as i64, hash],
            )
            .unwrap();

        assert_eq!(db.backfill_classifications(1).unwrap(), 1);
        assert_eq!(db.backfill_classifications(1).unwrap(), 0);

        let entries = db.get_all(None, None, 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, "website");
        assert_eq!(entries[0].categories, vec!["website", "text"]);
        assert_eq!(
            entries[0].classifier_version,
            crate::history_classifier::CLASSIFIER_VERSION
        );
        assert_eq!(db.get_all(None, Some("website"), 10, 0).unwrap().len(), 1);
        assert_eq!(db.get_all(None, Some("text"), 10, 0).unwrap().len(), 1);

        let encoded: String = db
            .conn
            .query_row("SELECT categories FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&encoded).unwrap(),
            vec!["website", "text"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_filters_use_secondary_labels_and_validate_date_ranges() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-combined-filter-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db = test_database(&root);
        db.conn
            .execute_batch(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, categories, category_confidence, classifier_version)
                 VALUES
                    ('2026-02-01T18:00:00+08:00', 'text', 'needle at start', X'00', 1,
                     'Mac', 'start', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T19:00:00+08:00', 'text', 'needle at end', X'00', 1,
                     'Mac', 'end', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T18:30:00+08:00', 'text', 'unrelated', X'00', 1,
                     'Mac', 'other', 'website', '[\"website\",\"text\"]', 93, 3),
                    ('2026-02-01T18:30:00+08:00', 'text', 'needle primary only', X'00', 1,
                     'self', 'primary', 'website', '[\"website\"]', 99, 3);",
            )
            .unwrap();

        let results = db
            .get_all_filtered(
                Some("needle"),
                Some("text"),
                Some("2026-02-01T10:00:00Z"),
                Some("2026-02-01T11:00:00Z"),
                10,
                0,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data_hash, "start");
        assert_eq!(results[0].categories, vec!["website", "text"]);

        let invalid = db
            .get_all_filtered(None, None, Some("2026-02-30T10:00:00Z"), None, 10, 0)
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("Invalid start_time"));

        let reversed = db
            .get_all_filtered(
                None,
                None,
                Some("2026-02-01T11:00:00Z"),
                Some("2026-02-01T10:00:00Z"),
                10,
                0,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(reversed, "start_time must be earlier than end_time");

        let equal = db
            .get_all_filtered(
                None,
                None,
                Some("2026-02-01T10:00:00Z"),
                Some("2026-02-01T10:00:00Z"),
                10,
                0,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(equal, "start_time must be earlier than end_time");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_entries_persist_categories_and_category_queries_page_stably() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-query-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        db.add_text("https://example.com/docs", "Mac").unwrap();
        db.add_text("const answer = 42;", "Mac").unwrap();
        db.add_text("ordinary note", "self").unwrap();
        db.add_image(b"not-a-real-image", "Mac").unwrap();
        db.add_file("report.pdf", b"file bytes", "Mac").unwrap();
        db.conn
            .execute("UPDATE history SET timestamp = '2026-01-01T00:00:00Z'", [])
            .unwrap();

        let websites = db.get_all(None, Some("website"), 10, 0).unwrap();
        let code = db.get_all(Some("Mac"), Some("code"), 10, 0).unwrap();
        let first_page = db.get_all(None, None, 2, 0).unwrap();
        let second_page = db.get_all(None, None, 2, 2).unwrap();
        let image_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let file_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'file'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(websites.len(), 1);
        assert_eq!(websites[0].category, "website");
        assert_eq!(code.len(), 1);
        assert_eq!(code[0].category, "code");
        assert!(first_page[0].id > first_page[1].id);
        assert!(first_page[1].id > second_page[0].id);
        assert_eq!(image_category, "image");
        assert_eq!(file_category, "file");
        assert!(db.get_all(None, Some("unsupported"), 10, 0).is_err());
        db.conn
            .execute(
                "UPDATE history
                 SET category = 'text', category_confidence = 0, classifier_version = 0
                 WHERE type = 'image'",
                [],
            )
            .unwrap();
        assert_eq!(db.backfill_classifications(10).unwrap(), 1);
        let repaired_image_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(repaired_image_category, "image");

        db.clear_all().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backfill_repairs_legacy_text_references_and_marks_corrupt_rows() {
        let root = std::env::temp_dir().join(format!(
            "tailsync-category-backfill-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut db = test_database(&root);
        let text = b"git status --short";
        let hash = blake3::hash(text).to_hex().to_string();
        let legacy_reference = persist_image_at(&db.image_history_dir, &hash, text).unwrap();
        let legacy_path = resolve_file_reference_at(
            &db.image_history_dir,
            &decode_image_reference(&legacy_reference).unwrap(),
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-01T00:00:00Z', 'text', 'git status --short', ?1, ?2, 'migrated', ?3)",
                params![legacy_reference, text.len() as i64, hash],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-02T00:00:00Z', 'text', 'broken', X'010203', 3, 'migrated', 'broken')",
                [],
            )
            .unwrap();
        let regular_text = b"example.com";
        let regular_hash = blake3::hash(regular_text).to_hex().to_string();
        let regular_encrypted = crypto::encrypt(regular_text).unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash,
                     category, category_confidence, classifier_version)
                 VALUES ('2026-01-03T00:00:00Z', 'text', 'website', ?1, ?2, 'self', ?3,
                         'text', 75, 1)",
                params![regular_encrypted, regular_text.len() as i64, regular_hash],
            )
            .unwrap();

        assert_eq!(db.backfill_classifications(50).unwrap(), 3);
        assert_eq!(db.backfill_classifications(50).unwrap(), 0);

        let repaired: (i64, String, i64, i64) = db
            .conn
            .query_row(
                "SELECT id, category, category_confidence, classifier_version
                 FROM history WHERE data_hash = ?1",
                params![hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let corrupt: (String, i64, i64) = db
            .conn
            .query_row(
                "SELECT category, category_confidence, classifier_version
                 FROM history WHERE data_hash = 'broken'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let regular_category: String = db
            .conn
            .query_row(
                "SELECT category FROM history WHERE data_hash = ?1",
                params![regular_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(repaired.1, "command");
        assert!(repaired.2 >= 90);
        assert_eq!(repaired.3, crate::history_classifier::CLASSIFIER_VERSION);
        assert_eq!(db.get_data(repaired.0).unwrap(), text);
        assert!(!legacy_path.exists());
        assert_eq!(regular_category, "website");
        assert_eq!(
            corrupt,
            (
                "text".to_string(),
                0,
                crate::history_classifier::CLASSIFIER_VERSION,
            )
        );

        let deletable_text = b"legacy text reference for deletion";
        let deletable_hash = blake3::hash(deletable_text).to_hex().to_string();
        let deletable_reference =
            persist_image_at(&db.image_history_dir, &deletable_hash, deletable_text).unwrap();
        let deletable_path = resolve_file_reference_at(
            &db.image_history_dir,
            &decode_image_reference(&deletable_reference).unwrap(),
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT INTO history
                    (timestamp, type, description, data, size_bytes, source_peer, data_hash)
                 VALUES ('2026-01-04T00:00:00Z', 'text', 'deletable', ?1, ?2, 'migrated', ?3)",
                params![
                    deletable_reference,
                    deletable_text.len() as i64,
                    deletable_hash
                ],
            )
            .unwrap();
        let deletable_id = db.conn.last_insert_rowid();
        assert!(deletable_path.is_file());
        db.delete(deletable_id).unwrap();
        assert!(!deletable_path.exists());

        db.clear_all().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
