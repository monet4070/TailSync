use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

pub const STORAGE_DIRECTORY_NAME: &str = "TailSync Data";

/// Return the platform application-data directory, creating it when needed.
/// Configuration, identity material, and key state always stay here.
pub fn get_data_dir() -> PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR
        .get_or_init(|| {
            let directory = std::env::var_os("TAILSYNC_DATA_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    directories::ProjectDirs::from("com", "tailsync", "TailSync")
                        .map(|dirs| dirs.data_dir().to_path_buf())
                })
                .unwrap_or_else(|| {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join(".tailsync")
                });
            // The default data directory is app-owned: lock it to the user.
            // (Custom storage roots configured later are deliberately left
            // untouched — only directories the app creates get restricted.)
            let _ = crate::private_fs::create_private_dir_all(&directory);
            directory
        })
        .clone()
}

fn storage_state() -> &'static RwLock<PathBuf> {
    static STORAGE_DIR: OnceLock<RwLock<PathBuf>> = OnceLock::new();
    STORAGE_DIR.get_or_init(|| {
        let initial = std::env::var_os("TAILSYNC_STORAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(get_data_dir);
        RwLock::new(initial)
    })
}

/// Configure the bulk-data directory. Call this after settings are loaded and
/// before opening HistoryDB. Passing None restores the system-data location.
pub fn configure_storage_dir(path: Option<&Path>) -> Result<PathBuf, String> {
    let directory = path.map(Path::to_path_buf).unwrap_or_else(get_data_dir);
    validate_storage_dir(&directory)?;
    let mut current = storage_state()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *current = directory.clone();
    drop(current);
    crate::private_fs::create_private_dir_all(&directory)
        .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    Ok(directory)
}

pub fn configure_storage_parent(parent: &Path) -> Result<PathBuf, String> {
    validate_storage_dir(parent)?;
    let child = parent.join(STORAGE_DIRECTORY_NAME);
    configure_storage_dir(Some(&child))
}

pub fn get_storage_dir() -> PathBuf {
    storage_state()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub fn validate_storage_dir(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("Storage location cannot be empty".to_string());
    }
    let value = path.to_string_lossy();
    if value.starts_with("\\\\") || value.starts_with("//") {
        return Err("Network storage locations are not supported".to_string());
    }
    validate_local_storage(path)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn validate_local_storage(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    const DRIVE_REMOTE: u32 = 4;

    let absolute = path
        .canonicalize()
        .or_else(|_| std::env::current_dir().map(|current| current.join(path)))
        .map_err(|error| format!("Could not resolve storage location: {error}"))?;
    let Some(Component::Prefix(prefix)) = absolute.components().next() else {
        return Ok(());
    };
    let drive = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("{}:\\", char::from(letter))
        }
        Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) => {
            return Err("Network storage locations are not supported".to_string())
        }
        _ => return Ok(()),
    };
    let wide = std::ffi::OsStr::new(&drive)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOTE {
        return Err("Network storage locations are not supported".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_local_storage(path: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;

    let existing = path
        .ancestors()
        .find(|candidate| candidate.exists())
        .unwrap_or(Path::new("/"));
    let canonical = existing
        .canonicalize()
        .map_err(|error| format!("Could not resolve storage location: {error}"))?;
    let c_path = std::ffi::CString::new(canonical.as_os_str().as_bytes())
        .map_err(|_| "Storage location contains an invalid null byte".to_string())?;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(c_path.as_ptr(), info.as_mut_ptr()) } != 0 {
        return Err(format!(
            "Could not inspect storage location: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { info.assume_init() }.f_flags & u32::try_from(libc::MNT_LOCAL).unwrap_or(0) == 0 {
        return Err("Network storage locations are not supported".to_string());
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn validate_local_storage(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub fn get_history_db_path() -> PathBuf {
    get_storage_dir().join("history-v2.db")
}

pub fn get_file_history_dir() -> PathBuf {
    get_storage_dir().join("file-history")
}

pub fn get_image_history_dir() -> PathBuf {
    get_storage_dir().join("image-history")
}

pub fn get_incoming_dir() -> PathBuf {
    get_storage_dir().join("incoming")
}

pub fn get_clipboard_files_dir() -> PathBuf {
    get_storage_dir().join("clipboard-files")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_parent_uses_fixed_directory_name() {
        let parent = std::env::temp_dir().join(format!(
            "tailsync-storage-parent-{:016x}",
            rand::random::<u64>()
        ));
        let selected = configure_storage_parent(&parent).unwrap();
        assert_eq!(selected, parent.join(STORAGE_DIRECTORY_NAME));
        let _ = std::fs::remove_dir_all(parent);
        configure_storage_dir(None).unwrap();
    }

    #[test]
    fn unc_storage_is_rejected() {
        assert!(validate_storage_dir(Path::new(r"\\server\share")).is_err());
    }
}
