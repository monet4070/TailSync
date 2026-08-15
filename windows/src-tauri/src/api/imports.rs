//! Platform adapter for the shared import session logic.
//!
//! Session bookkeeping, chunk streaming, hashing, and HistoryDB commits live
//! in `tailsync_core::import`; this module only maps the local API
//! `Request`/`ApiState` types onto the shared functions. The file is
//! byte-identical on both platforms (enforced by the cross-platform drift
//! check).

use super::*;
use tailsync_core::import::{
    append_import_chunk as core_append_import_chunk, begin_import as core_begin_import,
    commit_import as core_commit_import, finalize_import as core_finalize_import,
    import_size_limit as core_import_size_limit, BeginImportParams, BeginImportResult,
};

pub(super) fn import_size_limit(entry_type: &str) -> Result<u64, String> {
    core_import_size_limit(entry_type).map_err(|error| error.to_string())
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
    let params = BeginImportParams {
        time: req.time.clone().ok_or("missing time")?,
        entry_type: req.entry_type.clone().ok_or("missing type")?,
        description: req.desc.clone().ok_or("missing description")?,
        expected_size: req.total_size.ok_or("missing total_size")?,
        data_hash: req.data_hash.clone(),
    };
    let mut imports = state.imports.lock().await;
    core_begin_import(&mut imports, &db::get_incoming_dir(), &params)
        .map(
            |result: BeginImportResult| {
                serde_json::json!({ "import_id": result.import_id, "next_offset": result.next_offset })
            },
        )
        .map_err(|error| error.to_string())
}

pub(super) async fn append_import_chunk(req: &Request, state: &ApiState) -> Result<Value, String> {
    let import_id = req.import_id.as_deref().ok_or("missing import_id")?;
    let offset = req.import_offset.ok_or("missing import_offset")?;
    let chunk_b64 = req.chunk_b64.as_deref().ok_or("missing chunk_b64")?;
    let mut imports = state.imports.lock().await;
    core_append_import_chunk(&mut imports, import_id, offset, chunk_b64)
        .map(|next_offset| serde_json::json!({ "next_offset": next_offset }))
        .map_err(|error| error.to_string())
}

pub(super) async fn finish_import(req: &Request, state: &ApiState) -> Result<Value, String> {
    let import_id = req.import_id.as_deref().ok_or("missing import_id")?;
    let mut imports = state.imports.lock().await;
    let finished =
        core_finalize_import(&mut imports, import_id).map_err(|error| error.to_string())?;
    let commit_result = {
        let mut database = state.db.lock().await;
        core_commit_import(&mut database, &finished)
    };
    if let Some(path) = &finished.path {
        let _ = std::fs::remove_file(path);
    }
    if commit_result.is_ok() {
        bump_clipboard_version();
    }
    commit_result
        .map(|()| serde_json::json!({ "size": finished.size, "data_hash": finished.data_hash }))
        .map_err(|error| error.to_string())
}
