//! Authenticated, current-user local transport for the native WPF frontend.
//! No webview or localhost listener is used by this mode.
#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::{watch, Semaphore};

const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_BINARY_BYTES: usize = 70 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;

pub(crate) struct Configuration {
    pipe: String,
    token: String,
    isolated: bool,
    network_check: bool,
    parent_pid: Option<u32>,
}

static CONFIGURATION: OnceLock<Configuration> = OnceLock::new();

pub(crate) fn active() -> bool {
    CONFIGURATION.get().is_some()
}
pub(crate) fn isolated() -> bool {
    CONFIGURATION.get().is_some_and(|config| config.isolated)
}

/// Explicit two-process Iroh acceptance: real transport, isolated identities,
/// no clipboard, registry, discovery broadcast or system shortcut integration.
pub(crate) fn network_check() -> bool {
    CONFIGURATION
        .get()
        .is_some_and(|config| config.network_check)
}

fn valid_pipe_name(name: &str) -> bool {
    name.strip_prefix("TailSync.Wpf.")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Called before crypto/database startup. Secrets never appear in argv or logs.
pub(crate) fn configure() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().collect();
    if !arguments.iter().any(|argument| argument == "--wpf-backend") {
        return Ok(());
    }
    if !cfg!(target_os = "windows") {
        return Err("WPF backend requires Windows".into());
    }
    let pipe = arguments
        .iter()
        .find_map(|arg| arg.strip_prefix("--pipe="))
        .ok_or("WPF pipe name is required")?
        .to_string();
    if !valid_pipe_name(&pipe) {
        return Err("Invalid WPF pipe name".into());
    }
    let token = std::env::var("TAILSYNC_WPF_SESSION_TOKEN").unwrap_or_default();
    // Only setup code runs at this point. No spawned process inherits the secret.
    std::env::remove_var("TAILSYNC_WPF_SESSION_TOKEN");
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("WPF session token is required".into());
    }
    let parent_pid = arguments
        .iter()
        .find_map(|arg| arg.strip_prefix("--parent-pid="))
        .map(|pid| pid.parse::<u32>())
        .transpose()
        .map_err(|_| "Invalid parent process")?;
    #[cfg(target_os = "windows")]
    if let Some(pid) = parent_pid {
        windows_transport::validate_parent(pid)?;
    }
    let isolated = arguments
        .iter()
        .any(|argument| argument == "--wpf-isolated");
    let network_check = arguments
        .iter()
        .any(|argument| argument == "--wpf-network-check");
    if network_check && !isolated {
        return Err("WPF network acceptance requires an isolated profile".into());
    }
    if isolated
        && !std::env::var_os("TAILSYNC_DATA_DIR")
            .is_some_and(|path| std::path::Path::new(&path).is_absolute())
    {
        return Err("Isolated WPF backend requires an absolute TAILSYNC_DATA_DIR".into());
    }
    CONFIGURATION
        .set(Configuration {
            pipe,
            token,
            isolated,
            network_check,
            parent_pid,
        })
        .map_err(|_| "WPF backend was already configured".to_string())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeEvent {
    revision: u64,
    action: String,
    window: String,
}

struct EventQueue {
    revision: u64,
    events: VecDeque<NativeEvent>,
}

pub(crate) struct FrontendEvents {
    queue: Mutex<EventQueue>,
    changed: watch::Sender<u64>,
}

impl Default for FrontendEvents {
    fn default() -> Self {
        Self {
            queue: Mutex::new(EventQueue {
                revision: 0,
                events: VecDeque::new(),
            }),
            changed: watch::channel(0).0,
        }
    }
}

/// Return true when the native shell has taken ownership of this window action.
pub(crate) fn navigate<R: tauri::Runtime>(app: &AppHandle<R>, action: &str, window: &str) -> bool {
    if !active() {
        return false;
    }
    let Some(events) = app.try_state::<FrontendEvents>() else {
        return false;
    };
    #[cfg(target_os = "windows")]
    if matches!(action, "open" | "toggle") {
        // Tray/shortcut input belongs to this process; the native window lives
        // in our validated parent. Hand off activation before publishing it.
        windows_transport::allow_frontend_activation();
    }
    let mut queue = events
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.revision = queue.revision.saturating_add(1);
    let revision = queue.revision;
    queue.events.push_back(NativeEvent {
        revision,
        action: action.to_string(),
        window: window.to_string(),
    });
    while queue.events.len() > 128 {
        queue.events.pop_front();
    }
    events.changed.send_replace(revision);
    true
}

async fn wait_events(app: &AppHandle, since: u64, wait_ms: u64) -> Value {
    let events = app.state::<FrontendEvents>();
    let mut receiver = events.changed.subscribe();
    if *receiver.borrow_and_update() <= since {
        let _ = tokio::time::timeout(
            Duration::from_millis(wait_ms.clamp(50, 15_000)),
            receiver.changed(),
        )
        .await;
    }
    let queue = events
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    json!({ "revision": queue.revision, "events": queue.events.iter().filter(|event| event.revision > since).collect::<Vec<_>>() })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    token: String,
    command: String,
    #[serde(default = "empty_args")]
    args: Value,
}
fn empty_args() -> Value {
    json!({})
}

fn authenticated(expected: &str, actual: &str) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(actual.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn stable_error(code: tailsync_runtime::contracts::StableErrorCode) -> Value {
    serde_json::to_value(crate::commands::CommandError::code(code)).unwrap_or(Value::Null)
}
fn invalid_argument() -> Value {
    stable_error(tailsync_runtime::contracts::StableErrorCode::InvalidArgument)
}
fn argument<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<T, Value> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|_| invalid_argument())
}

struct Reply {
    result: Value,
    binary: Option<Vec<u8>>,
}
impl Reply {
    fn json<T: Serialize>(value: T) -> Result<Self, Value> {
        Ok(Self {
            result: serde_json::to_value(value).map_err(|_| {
                stable_error(tailsync_runtime::contracts::StableErrorCode::InternalError)
            })?,
            binary: None,
        })
    }
    fn binary(bytes: Vec<u8>) -> Result<Self, Value> {
        if bytes.len() > MAX_BINARY_BYTES {
            return Err(invalid_argument());
        }
        Ok(Self {
            result: Value::Null,
            binary: Some(bytes),
        })
    }
}

async fn dispatch(app: &AppHandle, command: &str, args: Value) -> Result<Reply, Value> {
    use crate::commands as c;
    use tauri::ipc::{InvokeResponseBody, IpcResponse};
    if !args.is_object() {
        return Err(invalid_argument());
    }
    // Isolated acceptance runs real storage/core commands but never publish to
    // the user's clipboard, alter OS shortcuts, or contact/disconnect peers.
    // Bundled guide IDs are an adapter-local namespace, never database IDs.
    // This also rejects a stale preview after the frontend has left the guide.
    fn virtual_argument(value: &Value) -> bool {
        match value {
            Value::Number(number) => number
                .as_i64()
                .is_some_and(|id| (8_000_000_000..9_000_000_000).contains(&id)),
            Value::String(text) => {
                text.starts_with("onboarding-demo-")
                    || text.starts_with("demo-batch")
                    || text == "tailsync-demo-batch"
            }
            Value::Array(items) => items.iter().any(virtual_argument),
            Value::Object(items) => items.values().any(virtual_argument),
            _ => false,
        }
    }
    if virtual_argument(&args) {
        return Err(invalid_argument());
    }
    if isolated()
        && matches!(
            command,
            "restore_entry"
                | "restore_file_batch"
                | "set_sync_shortcut"
                | "set_history_shortcut"
                | "suspend_sync_shortcut"
                | "resume_sync_shortcut"
                | "check_for_update"
                | "install_update"
                | "change_storage_location"
                | "delete_old_storage"
        )
    {
        return Err(stable_error(
            tailsync_runtime::contracts::StableErrorCode::UnsupportedFormat,
        ));
    }
    if isolated()
        && !network_check()
        && matches!(
            command,
            "refresh_peers"
                | "test_connection"
                | "start_pairing"
                | "start_remote_pairing"
                | "create_remote_pairing_invite"
                | "confirm_pairing"
        )
    {
        return Err(stable_error(
            tailsync_runtime::contracts::StableErrorCode::UnsupportedFormat,
        ));
    }
    if isolated() && !network_check() && command == "update_settings" {
        let patch: String = argument(&args, "settingsJson")?;
        let patch: Value = serde_json::from_str(&patch).map_err(|_| invalid_argument())?;
        if patch.get("connection_mode").is_some() {
            return Err(stable_error(
                tailsync_runtime::contracts::StableErrorCode::UnsupportedFormat,
            ));
        }
    }
    let state = app.state::<crate::AppState>();
    macro_rules! arg {
        ($name:literal) => {
            argument(&args, $name)?
        };
    }
    macro_rules! call {
        ($value:expr) => {
            Reply::json($value.map_err(|error| serde_json::to_value(error).unwrap_or(Value::Null))?)
        };
    }
    macro_rules! binary {
        ($value:expr) => {{
            let response =
                $value.map_err(|error| serde_json::to_value(error).unwrap_or(Value::Null))?;
            match response.body().map_err(|_| {
                stable_error(tailsync_runtime::contracts::StableErrorCode::InternalError)
            })? {
                InvokeResponseBody::Raw(bytes) => Reply::binary(bytes),
                InvokeResponseBody::Json(_) => Err(invalid_argument()),
            }
        }};
    }
    // Only main frontend commands and native shell lifecycle commands are exposed.
    // Path-taking commands retain the core's existing package/cleanup validation.
    match command {
        "seed_wpf_thumbnail_history" => {
            if !isolated() {
                return Err(stable_error(
                    tailsync_runtime::contracts::StableErrorCode::Unauthorized,
                ));
            }
            let database = state.db.clone();
            let seeded = tauri::async_runtime::spawn_blocking(move || -> Result<Value, String> {
                use tailsync_core::composite::{
                    Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
                };
                let mut database = database.blocking_lock();
                if !database
                    .get_all(None, None, 1, 0)
                    .map_err(|e| e.to_string())?
                    .is_empty()
                {
                    return Err("Thumbnail fixtures require an empty isolated database".into());
                }
                let (width, height) = (96u32, 64u32);
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for y in 0..height {
                    for x in 0..width {
                        rgba.extend_from_slice(&[
                            (35 + x * 2) as u8,
                            (70 + y * 2) as u8,
                            if x > y { 180 } else { 70 },
                            255,
                        ]);
                    }
                }
                let packed = crate::protocol::pack_rgba_image(width, height, &rgba)
                    .map_err(|e| e.to_string())?;
                database
                    .add_image(&packed, "WPF thumbnail fixture")
                    .map_err(|e| e.to_string())?;
                let png = tailsync_core::composite::packed_to_png(width, height, &rgba)
                    .map_err(|e| e.to_string())?;
                let bundle = PortableBundle {
                    representations: vec![Representation {
                        mime: Mime::Png,
                        bytes: png.clone(),
                    }],
                    resources: vec![],
                    bindings: vec![],
                    completeness: Completeness::Complete,
                };
                let image_id = database
                    .add_composite(&bundle, None, "WPF thumbnail fixture")
                    .map_err(|e| e.to_string())?;
                let html =
                    "<p>清晰的图文剪贴板 · Image and text</p><img src='tailsync-resource:1'>";
                let document = PortableBundle {
                    representations: vec![Representation {
                        mime: Mime::Html,
                        bytes: html.as_bytes().to_vec(),
                    }],
                    resources: vec![Resource { id: 1, png }],
                    bindings: vec![ResourceBinding {
                        representation: 0,
                        resource: 1,
                        offset: html.find("tailsync-resource:1").unwrap() as u32,
                        length: 19,
                    }],
                    completeness: Completeness::Complete,
                };
                let document_id = database
                    .add_composite(&document, None, "WPF thumbnail fixture")
                    .map_err(|e| e.to_string())?;
                database
                    .add_text("更大的字号，更清晰的历史列表。", "WPF thumbnail fixture")
                    .map_err(|e| e.to_string())?;
                crate::api::bump_clipboard_version();
                Ok(json!({"composite_image_id": image_id, "document_id": document_id}))
            })
            .await
            .map_err(|_| {
                stable_error(tailsync_runtime::contracts::StableErrorCode::InternalError)
            })?;
            call!(seeded)
        }
        "seed_wpf_test_history" => {
            if !isolated() {
                return Err(stable_error(
                    tailsync_runtime::contracts::StableErrorCode::Unauthorized,
                ));
            }
            let count: usize = arg!("count");
            if !(1..=100).contains(&count) {
                return Err(invalid_argument());
            }
            let database = state.db.clone();
            let seeded = tauri::async_runtime::spawn_blocking(move || -> Result<Value, c::CommandError> {
                let mut database = database.blocking_lock();
                if !database.get_all(None, None, 1, 0).map_err(|_| c::CommandError::code(tailsync_runtime::contracts::StableErrorCode::InternalError))?.is_empty() {
                    return Err(c::CommandError::code(tailsync_runtime::contracts::StableErrorCode::InvalidArgument));
                }
                database.set_max_history(100);
                for index in 0..count {
                    let result = match index % 7 {
                        0 => database.add_text(&format!("WPF 验收示例 {index}: an encrypted clipboard record. 中文与 English。"), "WPF Acceptance Fixture"),
                        1 => database.add_text(&format!("https://example.invalid/wpf-fixture/{index}"), "WPF Acceptance Fixture"),
                        2 => database.add_text(&format!("fn fixture_{index}() {{\n    println!(\"WPF acceptance {index}\");\n}}"), "WPF Acceptance Fixture"),
                        3 => database.add_text(&format!("git log --oneline --max-count={index}"), "WPF Acceptance Fixture"),
                        4 => database.add_text(&format!("{{\"fixture\":{index},\"name\":\"WPF验收\",\"active\":true}}"), "WPF Acceptance Fixture"),
                        5 => {
                            let mut rgba = [255u8;16]; rgba[0] = index as u8; rgba[4] = (index * 2) as u8;
                            let packed = crate::protocol::pack_rgba_image(2, 2, &rgba).map_err(|_| c::CommandError::code(tailsync_runtime::contracts::StableErrorCode::InternalError))?;
                            database.add_image(&packed, "WPF Acceptance Fixture")
                        }
                        _ => database.add_file(&format!("wpf-acceptance-{index}.txt"), format!("WPF synthetic file {index}\n中文文本").as_bytes(), "WPF Acceptance Fixture").map(|_| ()),
                    };
                    result.map_err(|_| c::CommandError::code(tailsync_runtime::contracts::StableErrorCode::InternalError))?;
                }
                crate::api::bump_clipboard_version();
                let entries = database.get_all(None, None, 100, 0).map_err(|_| c::CommandError::code(tailsync_runtime::contracts::StableErrorCode::InternalError))?;
                Ok(json!({"count":entries.len(),"ids":entries.iter().map(|entry|entry.id).collect::<Vec<_>>()}))
            }).await.map_err(|_| stable_error(tailsync_runtime::contracts::StableErrorCode::InternalError))?;
            call!(seeded)
        }
        "handshake" => Reply::json(json!({ "protocolVersion": 1, "nativeUiContractVersion": 2,
                "backendVersion": env!("CARGO_PKG_VERSION"), "transport": "windows-named-pipe", "frontend": "wpf",
                "isolated": isolated(), "networkEnabled": !isolated() || network_check(),
                "connectionModes": ["auto", "lan_only", "iroh_only", "tailscale_only"],
                "settingsContract": "field-patch-v1", "capabilities": c::get_local_capabilities() })),
        "wait_wpf_events" => Reply::json(
            wait_events(
                app,
                argument::<Option<u64>>(&args, "sinceRevision")?.unwrap_or_default(),
                argument::<Option<u64>>(&args, "waitMs")?.unwrap_or(2500),
            )
            .await,
        ),
        "shutdown_wpf_backend" => {
            let sender = state.shutdown.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let _ = sender.send(true);
            });
            Reply::json(())
        }
        "get_local_capabilities" => Reply::json(c::get_local_capabilities()),
        "get_history_capabilities" => call!(c::get_history_capabilities().await),
        "get_history_page" => call!(
            c::get_history_page(
                state,
                arg!("keyword"),
                arg!("category"),
                arg!("startTime"),
                arg!("endTime"),
                arg!("limit"),
                arg!("offset"),
                arg!("collection")
            )
            .await
        ),
        "get_history" => call!(
            c::get_history(
                state,
                arg!("keyword"),
                arg!("category"),
                arg!("startTime"),
                arg!("endTime"),
                arg!("limit"),
                arg!("offset"),
                arg!("collection")
            )
            .await
        ),
        "get_migration_diagnostics" => call!(c::get_migration_diagnostics(state).await),
        "get_image_data" => call!(c::get_image_data(state, arg!("id"), arg!("cacheKey")).await),
        "get_preview" => binary!(c::get_preview(state, arg!("id"), arg!("batchId")).await),
        "get_document_preview" => {
            binary!(c::get_document_preview(state, app.state(), arg!("request")).await)
        }
        "validate_document_preview" => call!(
            c::validate_document_preview(state, app.state(), arg!("request"), arg!("stamp")).await
        ),
        "cancel_document_preview" => {
            call!(c::cancel_document_preview(app.state(), arg!("request")))
        }
        "restore_entry" => call!(
            c::restore_entry(
                app.clone(),
                state,
                arg!("id"),
                arg!("restoreFormat"),
                arg!("restoreOriginalId"),
                arg!("restoreIdentity")
            )
            .await
        ),
        "restore_file_batch" => {
            call!(c::restore_file_batch(app.clone(), state, arg!("batchId")).await)
        }
        "set_history_favorite" => {
            call!(c::set_history_favorite(state, arg!("id"), arg!("favorite")).await)
        }
        "set_history_pinned" => {
            call!(c::set_history_pinned(state, arg!("id"), arg!("pinned")).await)
        }
        "delete_entry" => call!(c::delete_entry(state, arg!("id")).await),
        "delete_favorite_entry" => call!(c::delete_favorite_entry(state, arg!("id")).await),
        "clear_history" => call!(c::clear_history(state).await),
        "cancel_file_batch" => call!(c::cancel_file_batch(state, arg!("batchId")).await),
        "get_file_progress" => call!(c::get_file_progress().await),
        "get_version" => call!(c::get_version().await),
        "wait_runtime_snapshot" => call!(
            c::wait_runtime_snapshot(
                arg!("sinceRevision"),
                arg!("waitMs"),
                arg!("sinceNotificationId")
            )
            .await
        ),
        "get_sync_warning" => call!(c::get_sync_warning().await),
        "ack_sync_warning" => call!(c::ack_sync_warning(arg!("id")).await),
        "get_settings" => call!(c::get_settings(state).await),
        "update_settings" => {
            call!(c::update_settings(app.clone(), state, arg!("settingsJson")).await)
        }
        "get_sync_state" => call!(c::get_sync_state(state).await),
        "set_sync_enabled" => call!(c::set_sync_enabled(app.clone(), arg!("enabled")).await),
        "toggle_sync" => call!(c::toggle_sync(app.clone()).await),
        "set_sync_shortcut" => {
            call!(c::set_sync_shortcut(app.clone(), state, arg!("shortcut")).await)
        }
        "set_history_shortcut" => {
            call!(c::set_history_shortcut(app.clone(), state, arg!("shortcut")).await)
        }
        "suspend_sync_shortcut" => call!(c::suspend_sync_shortcut(app.clone())),
        "resume_sync_shortcut" => call!(c::resume_sync_shortcut(app.clone(), state).await),
        "get_peers" if isolated() => {
            // No health worker exists in acceptance mode; waiting for one would
            // leave every native devices view disabled for two seconds. Keep
            // the real identity/paired settings, and explicitly report why no
            // discovery was performed rather than fabricating online devices.
            let settings = state.settings.lock().await.clone();
            Reply::json(crate::api::peer_snapshot_data(
                &state.identity,
                &settings,
                Err("Network discovery disabled in isolated WPF acceptance mode".into()),
            ))
        }
        "get_peers" => call!(c::get_peers(state).await),
        "refresh_peers" => call!(c::refresh_peers(state).await),
        "test_connection" => call!(c::test_connection(arg!("address")).await),
        "toggle_peer" => call!(c::toggle_peer(state, arg!("hostname"), arg!("enabled")).await),
        "forget_peer" => call!(c::forget_peer(state, arg!("hostname")).await),
        "enable_pairing" => call!(c::enable_pairing(state).await),
        "get_pairing_status" => call!(c::get_pairing_status(state).await),
        "start_pairing" => call!(c::start_pairing(state, arg!("address")).await),
        "confirm_pairing" => call!(c::confirm_pairing(state).await),
        "cancel_pairing" => call!(c::cancel_pairing(state).await),
        "create_remote_pairing_invite" => call!(c::create_remote_pairing_invite(state).await),
        "inspect_remote_pairing_link" => call!(c::inspect_remote_pairing_link(arg!("link"))),
        "start_remote_pairing" => call!(c::start_remote_pairing(state, arg!("link")).await),
        "get_remote_pairing_invite_status" => call!(c::get_remote_pairing_invite_status(state)),
        "cancel_remote_pairing_invite" => call!(c::cancel_remote_pairing_invite(state).await),
        "take_pending_remote_pairing_link" => call!(c::take_pending_remote_pairing_link(state)),
        #[cfg(target_os = "windows")]
        "queue_remote_pairing_link" => {
            c::queue_remote_pairing_link(app, &argument::<String>(&args, "link")?)
                .map_err(|_| invalid_argument())?;
            Reply::json(())
        }
        "get_storage_status" => call!(c::get_storage_status(state).await),
        "change_storage_location" => {
            call!(c::change_storage_location(app.clone(), state, arg!("parent")).await)
        }
        "delete_old_storage" => call!(c::delete_old_storage(state, arg!("path")).await),
        "get_update_status" => call!(c::get_update_status().await),
        "check_for_update" => call!(c::check_for_update(app.clone()).await),
        // The existing updater feed installs only the Tauri product. Until the
        // release pipeline ships the WPF shell and backend atomically, applying
        // that installer would leave the two applications on different versions.
        "install_update" => Err(stable_error(
            tailsync_runtime::contracts::StableErrorCode::UnsupportedFormat,
        )),
        "list_themes_v2" => Reply::json(c::list_themes_v2().await),
        "get_local_theme_settings" => Reply::json(c::get_local_theme_settings().await),
        "set_local_theme_settings" => {
            call!(c::set_local_theme_settings(app.clone(), arg!("settings")).await)
        }
        "resolve_theme" => call!(
            c::resolve_theme(
                arg!("themeId"),
                arg!("mode"),
                arg!("platform"),
                arg!("highContrast")
            )
            .await
        ),
        "validate_theme" => {
            Reply::json(c::validate_theme(arg!("path"), arg!("mode"), arg!("highContrast")).await)
        }
        "install_theme" => call!(c::install_theme(arg!("path"), arg!("expectedDigest")).await),
        "update_theme" => {
            call!(c::update_theme(arg!("path"), arg!("expectedDigest"), arg!("options")).await)
        }
        "rollback_theme" => call!(c::rollback_theme(arg!("themeId")).await),
        "delete_theme_v2" => {
            call!(c::delete_theme_v2(app.clone(), arg!("themeId"), arg!("storageHandle")).await)
        }
        "get_theme_asset_slot" => {
            binary!(c::get_theme_asset_slot(arg!("themeId"), arg!("digest"), arg!("slot")).await)
        }
        "preview_theme_asset_slot" => {
            binary!(c::preview_theme_asset_slot(arg!("path"), arg!("digest"), arg!("slot")).await)
        }
        "open_history_window" => call!(c::open_history_window(app.clone()).await),
        "open_favorites_window" => call!(c::open_favorites_window(app.clone()).await),
        "open_settings_window" => call!(c::open_settings_window(app.clone()).await),
        "open_connections_window" => call!(c::open_connections_window(app.clone()).await),
        "close_history_window" => call!(c::close_history_window(app.clone()).await),
        "close_favorites_window" => call!(c::close_favorites_window(app.clone()).await),
        "close_settings_window" => call!(c::close_settings_window(app.clone())),
        "close_connections_window" => call!(c::close_connections_window(app.clone())),
        "open_onboarding_window" => {
            call!(crate::onboarding::open_onboarding_window(app.clone()).await)
        }
        "dismiss_onboarding" => call!(crate::onboarding::dismiss_onboarding(app.clone()).await),
        "suppress_onboarding" => call!(crate::onboarding::suppress_onboarding(app.clone()).await),
        "onboarding_suppressed" => Reply::json(crate::onboarding::onboarding_suppressed()),
        "onboarding_should_show" => Reply::json(crate::onboarding::should_show_on_start()),
        _ => Err(invalid_argument()),
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn start(app: AppHandle, shutdown: watch::Receiver<bool>) -> std::io::Result<()> {
    if active() {
        for (event, window) in [
            ("settings-changed", "settings"),
            ("sync-state-changed", "settings"),
            ("theme_changed", "theme"),
        ] {
            let handle = app.clone();
            app.listen(event, move |_| {
                navigate(&handle, "refresh", window);
            });
        }
    }
    windows_transport::start(app, shutdown)
}

#[cfg(target_os = "windows")]
mod windows_transport;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipe_paths_cannot_escape_the_native_namespace() {
        assert!(valid_pipe_name(
            "TailSync.Wpf.0123456789abcdef0123456789abcdef"
        ));
        for name in [
            "other",
            "TailSync.Wpf../x",
            "TailSync.Wpf.0123",
            "\\\\remote\\pipe\\anything",
        ] {
            assert!(!valid_pipe_name(name));
        }
    }
    #[test]
    fn authentication_rejects_missing_different_and_wrong_length_tokens() {
        let token = "a".repeat(64);
        assert!(authenticated(&token, &token));
        assert!(!authenticated(&token, ""));
        assert!(!authenticated(&token, &"b".repeat(64)));
        assert!(!authenticated(&token, &format!("{token}a")));
    }
    #[test]
    fn requests_reject_extra_transport_fields_and_typed_args_fail_closed() {
        assert!(serde_json::from_value::<Request>(
            json!({"id":1,"token":"x","command":"get_settings","extra":true})
        )
        .is_err());
        assert!(argument::<u64>(&json!({"id":"1"}), "id").is_err());
        assert!(argument::<bool>(&json!({}), "enabled").is_err());
        assert_eq!(
            argument::<Option<usize>>(&json!({}), "offset").unwrap(),
            None
        );
    }
}
