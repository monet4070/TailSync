//! File clipboard detection.
//! macOS: tiny compiled Swift helper (instant, no subprocess per call)
//! Windows: Win32 CF_HDROP via windows-sys

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::io::Read;
#[cfg(target_os = "macos")]
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const CLIPBOARD_HELPER_TIMEOUT: Duration = Duration::from_secs(2);

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
    let child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let (status, output) = match wait_for_child(child, CLIPBOARD_HELPER_TIMEOUT) {
        Ok(result) => result,
        Err(error) => {
            log::warn!("{error}");
            return None;
        }
    };
    if !status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output);
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
    let child = Command::new(bin)
        .arg("--write-files")
        .args(paths)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not run clipboard helper: {error}"))?;
    let (status, _) = wait_for_child(child, CLIPBOARD_HELPER_TIMEOUT)?;
    if !status.success() {
        return Err(format!("Clipboard helper exited with status {status}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_child(mut child: Child, timeout: Duration) -> Result<(ExitStatus, Vec<u8>), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = Vec::new();
                if let Some(mut stdout) = child.stdout.take() {
                    stdout.read_to_end(&mut output).map_err(|error| {
                        format!("Could not read clipboard helper output: {error}")
                    })?;
                }
                return Ok((status, output));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Clipboard helper timed out after {} ms",
                    timeout.as_millis()
                ));
            }
            Err(error) => return Err(format!("Could not wait for clipboard helper: {error}")),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_test_platform_has_no_file_clipboard_contents() {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert!(read_clipboard_files().is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolved_clipboard_helper_path_is_absolute_when_present() {
        if let Some(path) = resolve_clipboard_helper() {
            assert!(
                path.is_absolute(),
                "resolved helper path was relative: {path:?}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stuck_clipboard_helper_is_killed_after_timeout() {
        let child = Command::new("/bin/sh")
            .args(["-c", "sleep 5"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();

        let error = wait_for_child(child, Duration::from_millis(50)).unwrap_err();

        assert!(error.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
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
