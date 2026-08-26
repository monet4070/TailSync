#![cfg_attr(target_os = "macos", allow(dead_code))]

#[cfg(target_os = "macos")]
use std::io::Write;
use std::io::{Cursor, Read};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::sync::OnceLock;

use tauri::AppHandle;
use tauri_plugin_updater::{Builder, UpdaterExt};

const UPDATE_PUBLIC_KEY: &str = include_str!("../../../shared/updater.pub");
#[cfg(test)]
const RELEASE_UPDATE_PUBLIC_KEY: &str = include_str!("../../../shared/updater.pub");

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
const PACKAGE_METADATA_PATH: &str = "tailsync-update.json";
const MAX_PACKAGE_METADATA_BYTES: u64 = 16 * 1024;

#[derive(Debug, serde::Deserialize)]
struct PackageMetadata {
    schema: u8,
    product: String,
    version: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
}

pub fn plugin_builder() -> Builder {
    Builder::new().pubkey(UPDATE_PUBLIC_KEY.trim())
}

pub fn register_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

pub fn app_handle() -> Result<&'static AppHandle, String> {
    APP_HANDLE
        .get()
        .ok_or_else(|| "The update service is still starting".to_string())
}

pub fn public_key_configured() -> bool {
    !UPDATE_PUBLIC_KEY.trim().is_empty()
}

fn require_public_key() -> Result<(), String> {
    if public_key_configured() {
        Ok(())
    } else {
        Err("The updater trust anchor is missing from this build".to_string())
    }
}

pub async fn check_for_update(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    require_public_key()?;
    let updater = app.updater().map_err(|error| error.to_string())?;
    let update = updater.check().await.map_err(|error| error.to_string())?;
    Ok(update.map(|update| UpdateInfo {
        current_version: update.current_version,
        version: update.version,
        notes: update.body,
        published_at: update.date.map(|date| date.to_string()),
    }))
}

pub async fn install_available_update(app: &AppHandle) -> Result<bool, String> {
    require_public_key()?;
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(false);
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    validate_update_package(&bytes, &update.version)?;
    update.install(&bytes).map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(target_os = "macos")]
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct HelperResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(target_os = "macos")]
fn helper_response(result: Result<serde_json::Value, String>) -> HelperResponse {
    match result {
        Ok(data) => HelperResponse {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => HelperResponse {
            ok: false,
            data: None,
            error: Some(error),
        },
    }
}

/// Run the updater plugin in a short-lived Tauri process. The normal macOS
/// daemon deliberately has no WebView/AppHandle, while the updater plugin
/// still requires one to perform its signed download and installation.
#[cfg(target_os = "macos")]
pub fn run_macos_updater_helper(operation: &str) -> Result<(), Box<dyn std::error::Error>> {
    let operation = operation.to_string();
    tauri::Builder::default()
        .plugin(plugin_builder().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = match operation.as_str() {
                    "check" => check_for_update(&handle)
                        .await
                        .and_then(|update| serde_json::to_value(update).map_err(|e| e.to_string())),
                    "install" => install_available_update(&handle)
                        .await
                        .and_then(|installed| {
                            serde_json::to_value(installed).map_err(|e| e.to_string())
                        }),
                    _ => Err(format!("Unknown updater operation: {operation}")),
                };
                let response = helper_response(result);
                if let Ok(encoded) = serde_json::to_string(&response) {
                    println!("{encoded}");
                    let _ = std::io::stdout().flush();
                }
                handle.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn invoke_macos_updater_helper(operation: &str) -> Result<HelperResponse, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--tailsync-updater-helper")
        .arg(operation)
        .env_remove("TAILSYNC_PARENT_PID")
        .env_remove("TAILSYNC_API_TOKEN")
        .env_remove("TAILSYNC_API_TOKEN_STDIN")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let timeout = if operation == "install" {
        std::time::Duration::from_secs(600)
    } else {
        std::time::Duration::from_secs(30)
    };
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("Signed updater helper timed out during {operation}"))?
        .map_err(|error| format!("Could not start signed updater helper: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let encoded = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("Updater helper returned no response: {}", stderr.trim())
        })?;
    let response: HelperResponse = serde_json::from_str(encoded)
        .map_err(|error| format!("Invalid updater helper response: {error}"))?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "Updater helper failed".to_string()));
    }
    Ok(response)
}

#[cfg(target_os = "macos")]
pub async fn check_for_update_headless() -> Result<Option<UpdateInfo>, String> {
    let response = invoke_macos_updater_helper("check").await?;
    response
        .data
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("Invalid update metadata: {error}"))
}

#[cfg(target_os = "macos")]
pub async fn install_available_update_headless() -> Result<bool, String> {
    let response = invoke_macos_updater_helper("install").await?;
    response
        .data
        .and_then(|value| value.as_bool())
        .ok_or_else(|| "Updater helper returned no installation result".to_string())
}

fn validate_metadata(metadata: &[u8], expected_version: &str) -> Result<(), String> {
    let metadata: PackageMetadata = serde_json::from_slice(metadata)
        .map_err(|error| format!("Invalid signed update metadata: {error}"))?;
    if metadata.schema != 1 || metadata.product != "TailSync" {
        return Err("The signed update package is not a TailSync v1 package".to_string());
    }
    if metadata.version != expected_version {
        return Err(format!(
            "Refusing update downgrade or substitution: manifest version {expected_version}, package version {}",
            metadata.version
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn validate_update_package(bytes: &[u8], expected_version: &str) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Invalid signed Windows update archive: {error}"))?;
    let mut metadata = archive
        .by_name(PACKAGE_METADATA_PATH)
        .map_err(|_| "Signed Windows update metadata is missing".to_string())?;
    let mut data = Vec::new();
    metadata
        .by_ref()
        .take(MAX_PACKAGE_METADATA_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| format!("Could not read signed update metadata: {error}"))?;
    if data.len() as u64 > MAX_PACKAGE_METADATA_BYTES {
        return Err("Signed update metadata exceeds its size limit".to_string());
    }
    validate_metadata(&data, expected_version)
}

#[cfg(target_os = "macos")]
fn validate_update_package(bytes: &[u8], expected_version: &str) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("Invalid signed macOS update archive: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("Invalid update entry: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("Invalid update entry path: {error}"))?;
        if !path.ends_with(format!("Contents/Resources/{PACKAGE_METADATA_PATH}")) {
            continue;
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_PACKAGE_METADATA_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|error| format!("Could not read signed update metadata: {error}"))?;
        if data.len() as u64 > MAX_PACKAGE_METADATA_BYTES {
            return Err("Signed update metadata exceeds its size limit".to_string());
        }
        return validate_metadata(&data, expected_version);
    }
    Err("Signed macOS update metadata is missing".to_string())
}

#[cfg(target_os = "windows")]
pub fn spawn_automatic_update_check(app: AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    if !public_key_configured() {
        log::debug!("Automatic updates are disabled because the updater trust anchor is missing");
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let update = match check_for_update(&app).await {
            Ok(Some(update)) => update,
            Ok(None) => return,
            Err(error) => {
                log::warn!("Automatic update check failed: {error}");
                return;
            }
        };
        let prompt = match update
            .notes
            .as_deref()
            .filter(|notes| !notes.trim().is_empty())
        {
            Some(notes) => format!(
                "TailSync {} is available (current {}).\n\n{}",
                update.version, update.current_version, notes
            ),
            None => format!(
                "TailSync {} is available (current {}).",
                update.version, update.current_version
            ),
        };
        let install_handle = app.clone();
        app.dialog()
            .message(prompt)
            .title("TailSync update")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Install update".to_string(),
                "Later".to_string(),
            ))
            .show(move |accepted| {
                if !accepted {
                    return;
                }
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = install_available_update(&install_handle).await {
                        log::error!("Update installation failed: {error}");
                        install_handle
                            .dialog()
                            .message(format!("TailSync could not install the update:\n\n{error}"))
                            .title("Update failed")
                            .kind(MessageDialogKind::Error)
                            .show(|_| {});
                    }
                });
            });
    });
}

#[cfg(test)]
mod tests {
    use super::{
        validate_metadata, validate_update_package, RELEASE_UPDATE_PUBLIC_KEY, UPDATE_PUBLIC_KEY,
    };

    #[test]
    fn embedded_update_key_is_never_a_documented_placeholder() {
        assert!(!UPDATE_PUBLIC_KEY.contains("REPLACE_WITH"));
    }

    #[test]
    fn release_build_key_matches_the_checked_in_trust_anchor() {
        assert!(!UPDATE_PUBLIC_KEY.trim().is_empty());
        assert_eq!(UPDATE_PUBLIC_KEY.trim(), RELEASE_UPDATE_PUBLIC_KEY.trim());
    }

    #[test]
    fn signed_package_version_must_match_the_release_manifest() {
        let metadata = br#"{"schema":1,"product":"TailSync","version":"2.1.0"}"#;
        assert!(validate_metadata(metadata, "2.1.0").is_ok());
        let error = validate_metadata(metadata, "2.2.0").unwrap_err();
        assert!(error.contains("Refusing update downgrade or substitution"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_update_archive_contains_validated_signed_metadata() {
        use std::io::{Cursor, Write};

        let cursor = Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive
            .start_file("TailSync-2.1.0-Windows-x64-setup.exe", options)
            .unwrap();
        archive.write_all(b"MZ updater fixture").unwrap();
        archive
            .start_file(super::PACKAGE_METADATA_PATH, options)
            .unwrap();
        archive
            .write_all(br#"{"schema":1,"product":"TailSync","version":"2.1.0"}"#)
            .unwrap();
        let bytes = archive.finish().unwrap().into_inner();

        validate_update_package(&bytes, "2.1.0").unwrap();
        assert!(validate_update_package(&bytes, "2.2.0")
            .unwrap_err()
            .contains("Refusing update downgrade or substitution"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_update_archive_contains_validated_signed_metadata() {
        use std::io::Cursor;

        let metadata = br#"{"schema":1,"product":"TailSync","version":"2.1.0"}"#;
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(metadata.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                "TailSync.app/Contents/Resources/tailsync-update.json",
                Cursor::new(metadata),
            )
            .unwrap();
        let bytes = archive.into_inner().unwrap().finish().unwrap();

        validate_update_package(&bytes, "2.1.0").unwrap();
        assert!(validate_update_package(&bytes, "2.2.0")
            .unwrap_err()
            .contains("Refusing update downgrade or substitution"));
    }
}
