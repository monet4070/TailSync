use std::io::Write;
use std::path::{Path, PathBuf};

use crate::crypto;

use super::{file_encryption, get_clipboard_files_dir};

const FILE_REFERENCE_MAGIC: &[u8] = b"TSFILE1\0";
const IMAGE_REFERENCE_MAGIC: &[u8] = b"TSIMAGE1";
const MAX_STORED_ORIGINAL_NAME_BYTES: usize = 120;
pub(super) const FILE_HISTORY_BYTE_LIMIT: i64 = 5 * 1024 * 1024 * 1024;

pub(super) fn validate_history_file_size(size: u64) -> Result<(), Box<dyn std::error::Error>> {
    if size > FILE_HISTORY_BYTE_LIMIT as u64 {
        return Err("File exceeds the 5 GiB history limit".into());
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredFileReference {
    pub(super) version: u8,
    pub(super) file_name: String,
}

pub(super) fn encode_file_reference(
    file_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    encode_file_reference_version(file_name, 2)
}

pub(super) fn encode_file_reference_version(
    file_name: &str,
    version: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !matches!(version, 1 | 2) {
        return Err(format!("Unsupported history file reference version: {version}").into());
    }
    let mut encoded = FILE_REFERENCE_MAGIC.to_vec();
    encoded.extend_from_slice(&serde_json::to_vec(&StoredFileReference {
        version,
        file_name: file_name.to_string(),
    })?);
    Ok(encoded)
}

pub(super) fn encode_image_reference(
    file_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut encoded = IMAGE_REFERENCE_MAGIC.to_vec();
    encoded.extend_from_slice(&serde_json::to_vec(&StoredFileReference {
        version: 1,
        file_name: file_name.to_string(),
    })?);
    Ok(encoded)
}

pub(super) fn decode_file_reference(data: &[u8]) -> Option<StoredFileReference> {
    let json = data.strip_prefix(FILE_REFERENCE_MAGIC)?;
    let reference = serde_json::from_slice::<StoredFileReference>(json).ok()?;
    matches!(reference.version, 1 | 2).then_some(reference)
}

pub(super) fn decode_image_reference(data: &[u8]) -> Option<StoredFileReference> {
    let json = data.strip_prefix(IMAGE_REFERENCE_MAGIC)?;
    let reference = serde_json::from_slice::<StoredFileReference>(json).ok()?;
    (reference.version == 1).then_some(reference)
}

pub(super) fn resolve_file_reference_at(
    directory: &Path,
    reference: &StoredFileReference,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let relative = Path::new(&reference.file_name);
    if relative.components().count() != 1 || relative.file_name().is_none() {
        return Err("Invalid history file reference".into());
    }
    Ok(directory.join(relative))
}

pub(super) fn sanitize_history_file_name(original_name: &str) -> String {
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

pub(super) fn persist_history_file_at(
    directory: &Path,
    data_hash: &str,
    original_name: &str,
    data: &[u8],
) -> Result<(Vec<u8>, PathBuf), Box<dyn std::error::Error>> {
    crate::private_fs::create_private_dir_all(directory)?;
    let safe_name = sanitize_history_file_name(original_name);
    let file_name = format!("{data_hash}-{safe_name}");
    let target = directory.join(&file_name);

    let target_matches = target.is_file()
        && file_encryption::encrypted_file_matches(&target, data.len() as u64, data_hash)
            .unwrap_or(false);
    if !target_matches {
        file_encryption::encrypt_bytes_atomic(data, data_hash, &target)?;
    }

    Ok((encode_file_reference(&file_name)?, target))
}

pub(super) fn persist_image_at(
    directory: &Path,
    data_hash: &str,
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    crate::private_fs::create_private_dir_all(directory)?;
    let file_name = format!("{data_hash}.bin");
    let target = directory.join(&file_name);
    if !target.is_file() {
        let encrypted = crypto::encrypt(data)?;
        // write_private_file uses a random temporary suffix (a fixed
        // process-id name would collide between concurrent writers) and
        // creates the container owner-only before the atomic rename.
        crate::private_fs::write_private_file(&target, &encrypted)?;
    }
    encode_image_reference(&file_name)
}

pub(super) fn persist_history_file_from_path_at(
    directory: &Path,
    data_hash: &str,
    original_name: &str,
    source: &Path,
    size: u64,
    move_source: bool,
) -> Result<(Vec<u8>, PathBuf), Box<dyn std::error::Error>> {
    crate::private_fs::create_private_dir_all(directory)?;
    let safe_name = sanitize_history_file_name(original_name);
    let file_name = format!("{data_hash}-{safe_name}");
    let target = directory.join(&file_name);

    let target_matches = target.is_file()
        && file_encryption::encrypted_file_matches(&target, size, data_hash).unwrap_or(false);
    if !target_matches {
        file_encryption::encrypt_file_atomic(source, size, data_hash, &target)?;
        if move_source && source != target {
            std::fs::remove_file(source)?;
        }
    } else if move_source && source != target {
        let _ = std::fs::remove_file(source);
    }

    Ok((encode_file_reference(&file_name)?, target))
}

pub(super) fn materialize_clipboard_file_at(
    directory: &Path,
    source: &Path,
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    materialize_clipboard_file_at_inner(directory, source, original_name)
}

fn materialize_clipboard_file_at_inner(
    directory: &Path,
    source: &Path,
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !std::fs::symlink_metadata(source)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(format!("Clipboard source file is missing: {}", source.display()).into());
    }
    let safe_name = sanitize_history_file_name(original_name);
    let transfer_directory = directory.join(format!("{:016x}", rand::random::<u64>()));
    crate::private_fs::create_private_dir_all(&transfer_directory)?;
    let target = transfer_directory.join(safe_name);
    if file_encryption::is_encrypted_file(source)? {
        file_encryption::decrypt_file_to_path(source, &target)?;
    } else {
        // Never hard-link a user file into managed storage: permissions and
        // later writes are inode-wide and would mutate the original source.
        crate::private_fs::copy_private_file(source, &target)?;
    }
    Ok(target)
}

pub fn materialize_clipboard_file(
    source: &Path,
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    materialize_clipboard_file_at(&get_clipboard_files_dir(), source, original_name)
}

/// Materialize a file received from a peer and mark it as having remote origin.
///
/// Plaintext legacy sources are copied instead of hard-linked so the platform
/// marker cannot modify the history source through a shared inode.
pub fn materialize_remote_clipboard_file(
    source: &Path,
    original_name: &str,
    source_peer: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    materialize_remote_clipboard_file_at(
        &get_clipboard_files_dir(),
        source,
        original_name,
        source_peer,
    )
}

pub(super) fn materialize_remote_clipboard_file_at(
    directory: &Path,
    source: &Path,
    original_name: &str,
    source_peer: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let target = materialize_clipboard_file_at_inner(directory, source, original_name)?;
    mark_as_remote_origin(&target, source_peer);
    Ok(target)
}

fn mark_as_remote_origin(path: &Path, source_peer: &str) {
    let _ = source_peer;

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            log::warn!("Could not mark remote file origin: path contains a null byte");
            return;
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let value = format!(
            "0081;{timestamp:x};TailSync;{:032x}",
            rand::random::<u128>()
        );
        let Ok(value) = std::ffi::CString::new(value) else {
            log::warn!("Could not mark remote file origin: invalid quarantine value");
            return;
        };
        let name = b"com.apple.quarantine\0";
        let result = unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr().cast(),
                value.as_ptr().cast(),
                value.as_bytes().len(),
                0,
                0,
            )
        };
        if result != 0 {
            log::warn!(
                "Could not mark received file as remote on macOS: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut stream_path = path.as_os_str().to_os_string();
        stream_path.push(":Zone.Identifier");
        let stream_path = PathBuf::from(stream_path);
        let marker = b"[ZoneTransfer]\r\nZoneId=3\r\n";
        if let Err(error) = std::fs::write(stream_path, marker) {
            log::warn!("Could not mark received file as remote on Windows: {error}");
        }
    }
}

pub fn cleanup_clipboard_files(referenced: &[PathBuf], minimum_age: std::time::Duration) {
    let directory = get_clipboard_files_dir();
    let referenced = referenced
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect::<Vec<_>>();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if referenced.iter().any(|item| item.starts_with(&canonical)) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= minimum_age);
        if old_enough {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub(super) fn materialize_clipboard_bytes_at(
    directory: &Path,
    data: &[u8],
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let safe_name = sanitize_history_file_name(original_name);
    let transfer_directory = directory.join(format!("{:016x}", rand::random::<u64>()));
    crate::private_fs::create_private_dir_all(&transfer_directory)?;
    let target = transfer_directory.join(safe_name);
    let mut file = crate::private_fs::create_private_file(&target)?;
    file.write_all(data)?;
    file.flush()?;
    file.sync_all()?;
    Ok(target)
}

pub fn materialize_clipboard_bytes(
    data: &[u8],
    original_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    materialize_clipboard_bytes_at(&get_clipboard_files_dir(), data, original_name)
}
