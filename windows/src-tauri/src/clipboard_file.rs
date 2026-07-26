//! File clipboard detection.
//! macOS: tiny compiled Swift helper (instant, no subprocess per call)
//! Windows: Win32 CF_HDROP via windows-sys

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::process::Command;

/// Returns file paths from the clipboard, or None if no files.
pub fn read_clipboard_files() -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        read_files_macos()
    }

    #[cfg(target_os = "windows")]
    {
        read_files_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn read_files_macos() -> Option<Vec<PathBuf>> {
    // Instant: compiled Swift binary, no compilation overhead
    let bin = resolve_clipboard_helper()?;
    let output = Command::new(&bin).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

#[cfg(target_os = "macos")]
pub fn write_clipboard_files(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No file paths were provided for the clipboard".to_string());
    }
    let bin = resolve_clipboard_helper()
        .ok_or_else(|| "Bundled clipboard helper was not found".to_string())?;
    let status = Command::new(bin)
        .arg("--write-files")
        .args(paths)
        .status()
        .map_err(|error| format!("Could not run clipboard helper: {error}"))?;
    if !status.success() {
        return Err(format!("Clipboard helper exited with status {status}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn resolve_clipboard_helper() -> Option<PathBuf> {
    let candidates = [
        "src-tauri/clipboard-helper",
        "../src-tauri/clipboard-helper",
        "clipboard-helper",
    ];
    for rel in &candidates {
        let p = std::path::Path::new(rel);
        if let Ok(path) = p.canonicalize() {
            if path.is_file() {
                return Some(path);
            }
        }
    }
    // Try alongside the executable
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.parent()?.join("clipboard-helper");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════
// Windows — CF_HDROP
// ═══════════════════════════════════════════════════════════════════

#[cfg(target_os = "windows")]
fn read_files_windows() -> Option<Vec<PathBuf>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(15); // CF_HDROP
        if h.is_null() {
            CloseClipboard();
            return None;
        }
        let drop_handle = h as HDROP;
        let count = DragQueryFileW(drop_handle, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        if count == 0 {
            CloseClipboard();
            return None;
        }
        let mut files = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(drop_handle, i, std::ptr::null_mut(), 0) as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop_handle, i, buf.as_mut_ptr(), buf.len() as u32);
            if let Some(null_idx) = buf.iter().position(|&c| c == 0) {
                buf.truncate(null_idx);
            }
            files.push(PathBuf::from(OsString::from_wide(&buf)));
        }
        CloseClipboard();
        if files.is_empty() {
            None
        } else {
            Some(files)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_test_platform_has_no_file_clipboard_contents() {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert!(super::read_clipboard_files().is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolved_clipboard_helper_path_is_absolute_when_present() {
        if let Some(path) = super::resolve_clipboard_helper() {
            assert!(
                path.is_absolute(),
                "resolved helper path was relative: {path:?}"
            );
        }
    }
}
