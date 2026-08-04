use super::*;

struct ImportSession {
    time: String,
    entry_type: String,
    description: String,
    expected_size: u64,
    expected_hash: Option<String>,
    received: u64,
    path: PathBuf,
    file: File,
    hasher: blake3::Hasher,
    updated_at: Instant,
}

#[derive(Default)]
pub(crate) struct ImportRegistry {
    sessions: HashMap<String, ImportSession>,
}

impl ImportRegistry {
    fn prune(&mut self, now: Instant) {
        let expired = self
            .sessions
            .iter()
            .filter(|(_, session)| now.duration_since(session.updated_at) > IMPORT_SESSION_TTL)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            if let Some(session) = self.sessions.remove(&id) {
                let _ = std::fs::remove_file(session.path);
            }
        }
    }
}

pub(super) fn import_size_limit(entry_type: &str) -> Result<u64, String> {
    match entry_type {
        "text" => Ok(crate::protocol::MAX_TEXT_PAYLOAD_SIZE as u64),
        "image" => Ok(crate::protocol::MAX_IMAGE_PAYLOAD_SIZE as u64),
        "file" => Ok(MAX_IMPORT_FILE_SIZE),
        _ => Err("unknown import type".to_string()),
    }
}

pub(super) fn import_response(result: Result<Value, String>) -> Response {
    match result {
        Ok(data) => Response {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => Response {
            ok: false,
            data: None,
            error: Some(error),
        },
    }
}

pub(super) async fn begin_import(req: &Request, state: &ApiState) -> Result<Value, String> {
    let time = req.time.as_deref().ok_or("missing time")?;
    chrono::DateTime::parse_from_rfc3339(time).map_err(|_| "invalid import timestamp")?;
    let entry_type = req.entry_type.as_deref().ok_or("missing type")?;
    let description = req.desc.as_deref().ok_or("missing description")?;
    if description.len() > 1024 {
        return Err("import description exceeds 1024 bytes".to_string());
    }
    let expected_size = req.total_size.ok_or("missing total_size")?;
    let limit = import_size_limit(entry_type)?;
    if expected_size > limit {
        return Err(format!(
            "{entry_type} import exceeds the {limit} byte limit"
        ));
    }
    let expected_hash = req
        .data_hash
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(|hash| {
            if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                Ok(hash.to_ascii_lowercase())
            } else {
                Err("data_hash must be 64 hexadecimal characters".to_string())
            }
        })
        .transpose()?;

    let directory = db::get_incoming_dir();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let now = Instant::now();
    let mut imports = state.imports.lock().await;
    imports.prune(now);
    if imports.sessions.len() >= API_MAX_IMPORTS {
        return Err(format!("active import limit ({API_MAX_IMPORTS}) reached"));
    }

    for _ in 0..8 {
        let import_id = hex::encode(rand::random::<[u8; 16]>());
        let path = directory.join(format!("api-import-{import_id}.part"));
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("could not create import file: {error}")),
        };
        imports.sessions.insert(
            import_id.clone(),
            ImportSession {
                time: time.to_string(),
                entry_type: entry_type.to_string(),
                description: description.to_string(),
                expected_size,
                expected_hash,
                received: 0,
                path,
                file,
                hasher: blake3::Hasher::new(),
                updated_at: now,
            },
        );
        return Ok(serde_json::json!({ "import_id": import_id, "next_offset": 0 }));
    }
    Err("could not allocate a unique import session".to_string())
}

pub(super) async fn append_import_chunk(req: &Request, state: &ApiState) -> Result<Value, String> {
    let import_id = req.import_id.as_deref().ok_or("missing import_id")?;
    if import_id.len() != 32 || !import_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid import_id".to_string());
    }
    let offset = req.import_offset.ok_or("missing import_offset")?;
    let encoded = req.chunk_b64.as_deref().ok_or("missing chunk_b64")?;
    let max_encoded = IMPORT_CHUNK_MAX_BYTES.div_ceil(3) * 4;
    if encoded.len() > max_encoded {
        return Err(format!(
            "import chunk exceeds the {IMPORT_CHUNK_MAX_BYTES} byte limit"
        ));
    }
    use base64::Engine;
    let chunk = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid chunk base64: {error}"))?;
    if chunk.is_empty() || chunk.len() > IMPORT_CHUNK_MAX_BYTES {
        return Err(format!(
            "import chunk must contain 1 to {IMPORT_CHUNK_MAX_BYTES} bytes"
        ));
    }

    let now = Instant::now();
    let mut imports = state.imports.lock().await;
    imports.prune(now);
    let session = imports
        .sessions
        .get_mut(import_id)
        .ok_or("unknown or expired import session")?;
    if offset != session.received {
        return Err(format!(
            "unexpected import offset {offset}; expected {}",
            session.received
        ));
    }
    let next_offset = offset
        .checked_add(chunk.len() as u64)
        .ok_or("import offset overflow")?;
    if next_offset > session.expected_size {
        return Err("import chunk exceeds declared total_size".to_string());
    }
    session
        .file
        .write_all(&chunk)
        .map_err(|error| format!("could not write import chunk: {error}"))?;
    session.hasher.update(&chunk);
    session.received = next_offset;
    session.updated_at = now;
    Ok(serde_json::json!({ "next_offset": next_offset }))
}

pub(super) async fn finish_import(req: &Request, state: &ApiState) -> Result<Value, String> {
    let import_id = req.import_id.as_deref().ok_or("missing import_id")?;
    let mut session = {
        let mut imports = state.imports.lock().await;
        imports.prune(Instant::now());
        imports
            .sessions
            .remove(import_id)
            .ok_or("unknown or expired import session")?
    };

    let result = (|| -> Result<(String, u64), String> {
        session
            .file
            .flush()
            .and_then(|_| session.file.sync_all())
            .map_err(|error| format!("could not finalize import file: {error}"))?;
        if session.received != session.expected_size {
            return Err(format!(
                "incomplete import: received {}, expected {} bytes",
                session.received, session.expected_size
            ));
        }
        let actual_hash = session.hasher.finalize().to_hex().to_string();
        if session
            .expected_hash
            .as_deref()
            .is_some_and(|hash| hash != actual_hash)
        {
            return Err("import data hash mismatch".to_string());
        }
        Ok((actual_hash, session.received))
    })();

    let (actual_hash, size) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = std::fs::remove_file(&session.path);
            return Err(error);
        }
    };
    drop(session.file);

    let inline_data = if session.entry_type == "file" {
        None
    } else {
        match std::fs::read(&session.path) {
            Ok(data) => Some(data),
            Err(error) => {
                let _ = std::fs::remove_file(&session.path);
                return Err(format!("could not read completed import: {error}"));
            }
        }
    };
    let db_result = {
        let mut database = state.db.lock().await;
        match session.entry_type.as_str() {
            "file" => database.add_file_migrated_from_path(
                &session.time,
                &session.description,
                &session.path,
                &actual_hash,
                size,
            ),
            "text" | "image" => match inline_data.as_deref() {
                Some(data) if session.entry_type == "text" => {
                    database.add_text_migrated(&session.time, &session.description, data)
                }
                Some(data) => {
                    database.add_image_migrated(&session.time, &session.description, data)
                }
                None => Err("completed import data is unavailable".into()),
            },
            _ => Err("unknown import type".into()),
        }
        .map_err(|error| error.to_string())
    };
    let _ = std::fs::remove_file(&session.path);
    db_result?;
    bump_clipboard_version();
    Ok(serde_json::json!({ "size": size, "data_hash": actual_hash }))
}
