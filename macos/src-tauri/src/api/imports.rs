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
use tailsync_runtime::execution::run_db;

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
    drop(imports);
    let response_size = finished.size;
    let response_hash = finished.data_hash.clone();
    struct RemoveCompletedImport(Option<std::path::PathBuf>);
    impl Drop for RemoveCompletedImport {
        fn drop(&mut self) {
            if let Some(path) = &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let cleanup = RemoveCompletedImport(finished.path.clone());
    let commit_result = run_db(state.db.clone(), move |database| {
        let _cleanup = cleanup;
        let result = core_commit_import(database, &finished);
        if result.is_ok() {
            bump_clipboard_version();
        }
        result
    })
    .await
    .map_err(|error| error.to_string());
    commit_result
        .map(|()| serde_json::json!({ "size": response_size, "data_hash": response_hash }))
        .map_err(|error| error.to_string())
}
