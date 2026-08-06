//! File clipboard detection.
//! macOS: tiny compiled Swift helper for native pasteboard representations
//! Windows: Win32 CF_HDROP via windows-sys

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::fs::{self, File};
#[cfg(target_os = "macos")]
use std::io::Read;
#[cfg(target_os = "macos")]
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
const CLIPBOARD_HELPER_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
const CLIPBOARD_IMAGE_HELPER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct ClipboardImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(target_os = "macos")]
fn parse_native_clipboard_image(output: &[u8]) -> Result<ClipboardImageData, String> {
    if output.len() <= 8 {
        return Err("Native clipboard image payload is incomplete".to_string());
    }
    let width = u32::from_le_bytes(
        output[0..4]
            .try_into()
            .map_err(|_| "Native clipboard image width is invalid")?,
    );
    let height = u32::from_le_bytes(
        output[4..8]
            .try_into()
            .map_err(|_| "Native clipboard image height is invalid")?,
    );
    let rgba_length = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Native clipboard image dimensions overflowed".to_string())?;
    let packed_length = rgba_length
        .checked_add(8)
        .ok_or_else(|| "Native clipboard image size overflowed".to_string())?;
    if width == 0 || height == 0 || packed_length > crate::protocol::MAX_IMAGE_PAYLOAD_SIZE {
        return Err("Native clipboard image exceeds the TailSync payload limit".to_string());
    }

    let decoded = image::load_from_memory_with_format(&output[8..], image::ImageFormat::Png)
        .map_err(|error| format!("Could not decode native clipboard image: {error}"))?;
    if decoded.width() != width || decoded.height() != height {
        return Err("Native clipboard image dimensions do not match its payload".to_string());
    }
    let rgba = decoded.into_rgba8().into_raw();
    if rgba.len() != rgba_length {
        return Err("Native clipboard image RGBA length is invalid".to_string());
    }
    Ok(ClipboardImageData {
        width,
        height,
        rgba,
    })
}

#[cfg(target_os = "macos")]
pub fn clipboard_files_are_readable(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            format!("Cannot inspect clipboard file {}: {error}", path.display())
        })?;
        if !metadata.is_file() {
            continue;
        }
        File::open(path)
            .map_err(|error| format!("Cannot read clipboard file {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn read_clipboard_image() -> Result<ClipboardImageData, String> {
    let bin = resolve_clipboard_helper()
        .ok_or_else(|| "Bundled clipboard helper was not found".to_string())?;
    let child = Command::new(bin)
        .arg("--read-image")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not run clipboard image helper: {error}"))?;
    let (status, output) = wait_for_child(child, CLIPBOARD_IMAGE_HELPER_TIMEOUT)?;
    if !status.success() {
        return Err(format!(
            "Clipboard image helper exited with status {status}"
        ));
    }
    parse_native_clipboard_image(&output)
}

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
    let mut output_reader = child.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output)?;
            Ok::<_, std::io::Error>(output)
        })
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = collect_child_output(output_reader.take())?;
                return Ok((status, output));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = collect_child_output(output_reader.take());
                return Err(format!(
                    "Clipboard helper timed out after {} ms",
                    timeout.as_millis()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = collect_child_output(output_reader.take());
                return Err(format!("Could not wait for clipboard helper: {error}"));
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_child_output(
    reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>,
) -> Result<Vec<u8>, String> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    reader
        .join()
        .map_err(|_| "Clipboard helper output reader panicked".to_string())?
        .map_err(|error| format!("Could not read clipboard helper output: {error}"))
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
#[allow(clippy::items_after_test_module)]
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

    #[cfg(target_os = "macos")]
    #[test]
    fn stuck_clipboard_helper_is_killed_after_timeout() {
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();

        let error = super::wait_for_child(child, std::time::Duration::from_millis(50)).unwrap_err();

        assert!(error.contains("timed out"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unreadable_clipboard_file_can_fall_back_to_another_representation() {
        let missing = std::env::temp_dir().join(format!(
            "tailsync-missing-clipboard-file-{:016x}",
            rand::random::<u64>()
        ));

        let error = super::clipboard_files_are_readable(&[missing]).unwrap_err();

        assert!(error.contains("Cannot inspect clipboard file"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn readable_clipboard_file_keeps_file_semantics() {
        let path = std::env::temp_dir().join(format!(
            "tailsync-readable-clipboard-file-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::write(&path, b"clipboard file").unwrap();

        let result = super::clipboard_files_are_readable(std::slice::from_ref(&path));

        std::fs::remove_file(path).unwrap();
        assert!(result.is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_clipboard_image_payload_decodes_to_rgba() {
        let png = include_bytes!("../icons/32x32.png");
        let mut payload = Vec::with_capacity(8 + png.len());
        payload.extend_from_slice(&32_u32.to_le_bytes());
        payload.extend_from_slice(&32_u32.to_le_bytes());
        payload.extend_from_slice(png);

        let image = super::parse_native_clipboard_image(&payload).unwrap();

        assert_eq!((image.width, image.height), (32, 32));
        assert_eq!(image.rgba.len(), 32 * 32 * 4);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_clipboard_image_rejects_mismatched_dimensions() {
        let png = include_bytes!("../icons/32x32.png");
        let mut payload = Vec::with_capacity(8 + png.len());
        payload.extend_from_slice(&31_u32.to_le_bytes());
        payload.extend_from_slice(&32_u32.to_le_bytes());
        payload.extend_from_slice(png);

        let error = super::parse_native_clipboard_image(&payload).unwrap_err();

        assert!(error.contains("dimensions do not match"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_clipboard_image_rejects_oversized_dimensions_before_decoding() {
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&4096_u32.to_le_bytes());
        payload.extend_from_slice(&4096_u32.to_le_bytes());
        payload.push(0);

        let error = super::parse_native_clipboard_image(&payload).unwrap_err();

        assert!(error.contains("payload limit"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn helper_output_larger_than_the_pipe_buffer_is_collected() {
        let child = std::process::Command::new("/usr/bin/head")
            .args(["-c", "262144", "/dev/zero"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let (status, output) =
            super::wait_for_child(child, std::time::Duration::from_secs(1)).unwrap();

        assert!(status.success());
        assert_eq!(output.len(), 262_144);
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

#[cfg(target_os = "windows")]
pub fn write_clipboard_files(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

    if paths.is_empty() {
        return Err("No file paths were provided for the clipboard".to_string());
    }
    let mut wide_paths = Vec::<u16>::new();
    for path in paths {
        wide_paths.extend(path.as_os_str().encode_wide());
        wide_paths.push(0);
    }
    wide_paths.push(0);
    let header_size = std::mem::size_of::<DropFilesHeader>();
    let total_size = header_size + wide_paths.len() * std::mem::size_of::<u16>();
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("Could not open the Windows clipboard".to_string());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("Could not clear the Windows clipboard".to_string());
        }
        let handle = GlobalAlloc(GHND, total_size);
        if handle.is_null() {
            CloseClipboard();
            return Err("Could not allocate Windows clipboard memory".to_string());
        }
        let pointer = GlobalLock(handle) as *mut u8;
        if pointer.is_null() {
            GlobalFree(handle);
            CloseClipboard();
            return Err("Could not lock Windows clipboard memory".to_string());
        }
        let header = DropFilesHeader {
            p_files: header_size as u32,
            pt: [0, 0],
            f_nc: 0,
            f_wide: 1,
        };
        std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, pointer, header_size);
        std::ptr::copy_nonoverlapping(
            wide_paths.as_ptr(),
            pointer.add(header_size) as *mut u16,
            wide_paths.len(),
        );
        GlobalUnlock(handle);
        if SetClipboardData(15, handle).is_null() {
            GlobalFree(handle);
            CloseClipboard();
            return Err("Could not publish files to the Windows clipboard".to_string());
        }
        CloseClipboard();
    }
    Ok(())
}

#[repr(C)]
#[cfg(target_os = "windows")]
struct DropFilesHeader {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}
