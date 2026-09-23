//! File clipboard detection.
//! macOS: tiny compiled Swift helper (instant, no subprocess per call)
//! Windows: Win32 CF_HDROP via windows-sys

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardFileReadError {
    Busy { attempts: u32, elapsed_ms: u64 },
    Win32 { code: u32 },
    InvalidData { code: u32 },
}

impl std::fmt::Display for ClipboardFileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy {
                attempts,
                elapsed_ms,
            } => write!(
                f,
                "Windows clipboard remained busy after {attempts} attempts over {elapsed_ms}ms"
            ),
            Self::Win32 { code } => write!(f, "Windows clipboard read failed with error {code}"),
            Self::InvalidData { code } => {
                write!(f, "Windows clipboard file data was invalid (error {code})")
            }
        }
    }
}

const CLIPBOARD_READ_DEADLINE: std::time::Duration = std::time::Duration::from_millis(750);

#[cfg(all(test, target_os = "windows"))]
static TEST_READ_ATTEMPTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn clipboard_retry_delay(attempt: u32) -> std::time::Duration {
    match attempt {
        0 => std::time::Duration::from_millis(10),
        1 => std::time::Duration::from_millis(20),
        2 => std::time::Duration::from_millis(40),
        3 => std::time::Duration::from_millis(80),
        4 => std::time::Duration::from_millis(120),
        5 => std::time::Duration::from_millis(160),
        _ => std::time::Duration::from_millis(200),
    }
}

#[cfg(target_os = "macos")]
use std::process::Command;

/// Returns file paths from the clipboard, or `Ok(None)` if no files are present.
/// A temporary Windows clipboard failure is returned as an error so callers do
/// not incorrectly fall back to broadcasting a file path as text.
pub fn read_clipboard_files() -> Result<Option<Vec<PathBuf>>, ClipboardFileReadError> {
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
        Ok(None)
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
fn read_files_windows() -> Result<Option<Vec<PathBuf>>, ClipboardFileReadError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

    let started = std::time::Instant::now();
    let mut attempts = 0u32;
    unsafe {
        loop {
            attempts += 1;
            #[cfg(test)]
            TEST_READ_ATTEMPTS.store(attempts, std::sync::atomic::Ordering::Relaxed);
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                break;
            }
            let code = GetLastError();
            if !is_retryable_clipboard_error(code) {
                return Err(ClipboardFileReadError::Win32 { code });
            }
            let elapsed = started.elapsed();
            if elapsed >= CLIPBOARD_READ_DEADLINE {
                return Err(ClipboardFileReadError::Busy {
                    attempts,
                    elapsed_ms: elapsed.as_millis() as u64,
                });
            }
            std::thread::sleep(clipboard_retry_delay(attempts - 1));
        }
        let h = GetClipboardData(15); // CF_HDROP
        if h.is_null() {
            CloseClipboard();
            return Ok(None);
        }
        let drop_handle = h as HDROP;
        let count = DragQueryFileW(drop_handle, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        if count == 0 {
            CloseClipboard();
            return Err(ClipboardFileReadError::InvalidData {
                code: GetLastError(),
            });
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
            Ok(None)
        } else {
            Ok(Some(files))
        }
    }
}

#[cfg(target_os = "windows")]
fn is_retryable_clipboard_error(code: u32) -> bool {
    // OpenClipboard commonly reports ERROR_ACCESS_DENIED while another process
    // owns the clipboard. The other values cover transient sharing/lock states;
    // zero is also treated as retryable because some clipboard owners do not
    // set last-error consistently.
    matches!(code, 0 | 5 | 32 | 33 | 170)
}

#[cfg(target_os = "windows")]
pub fn write_clipboard_files(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
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
        let _clipboard = open_clipboard_with_retry()?;
        if EmptyClipboard() == 0 {
            return Err("Could not clear the Windows clipboard".to_string());
        }
        let handle = GlobalAlloc(GHND, total_size);
        if handle.is_null() {
            return Err("Could not allocate Windows clipboard memory".to_string());
        }
        let pointer = GlobalLock(handle) as *mut u8;
        if pointer.is_null() {
            GlobalFree(handle);
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
            return Err("Could not publish files to the Windows clipboard".to_string());
        }
    }
    Ok(())
}

/// Write an RGBA image using CF_DIB when the high-level clipboard backend is
/// temporarily unavailable. All Win32 failures are returned to the caller;
/// successful SetClipboardData transfers ownership of the global handle.
#[cfg(target_os = "windows")]
pub fn write_clipboard_image(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

    let dib = rgba_to_dib(rgba, width, height)?;
    unsafe {
        let _clipboard = open_clipboard_with_retry()?;
        if EmptyClipboard() == 0 {
            return Err("Could not clear the Windows clipboard".to_string());
        }
        let handle = GlobalAlloc(GHND, dib.len());
        if handle.is_null() {
            return Err("Could not allocate Windows clipboard memory".to_string());
        }
        let pointer = GlobalLock(handle) as *mut u8;
        if pointer.is_null() {
            GlobalFree(handle);
            return Err("Could not lock Windows clipboard memory".to_string());
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), pointer, dib.len());
        GlobalUnlock(handle);
        if SetClipboardData(8, handle).is_null() {
            GlobalFree(handle);
            return Err("Could not publish the image to the Windows clipboard".to_string());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_clipboard_with_retry() -> Result<ClipboardGuard, String> {
    use windows_sys::Win32::System::DataExchange::OpenClipboard;
    const ATTEMPTS: usize = 10;
    const DELAY: std::time::Duration = std::time::Duration::from_millis(20);
    for attempt in 0..ATTEMPTS {
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                return Ok(ClipboardGuard);
            }
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(DELAY);
        }
    }
    Err("Could not open the Windows clipboard after bounded retries".to_string())
}

#[cfg(target_os = "windows")]
struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::DataExchange::CloseClipboard();
        }
    }
}

#[cfg(target_os = "windows")]
fn rgba_to_dib(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let row_size = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "Image dimensions are too large".to_string())?;
    let pixel_data_size = row_size
        .checked_mul(usize::try_from(height).map_err(|_| "Image height is too large")?)
        .ok_or_else(|| "Image dimensions are too large".to_string())?;
    if rgba.len() != pixel_data_size {
        return Err("Image RGBA data does not match its dimensions".to_string());
    }
    let dib_size = 40usize
        .checked_add(pixel_data_size)
        .ok_or_else(|| "Image dimensions are too large".to_string())?;
    let width_i32 = i32::try_from(width).map_err(|_| "Image width is too large")?;
    let height_i32 = i32::try_from(height).map_err(|_| "Image height is too large")?;
    let pixel_data_size_u32 =
        u32::try_from(pixel_data_size).map_err(|_| "Image dimensions are too large")?;
    let mut dib = Vec::with_capacity(dib_size);
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&width_i32.to_le_bytes());
    dib.extend_from_slice(&height_i32.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&pixel_data_size_u32.to_le_bytes());
    dib.extend_from_slice(&2835i32.to_le_bytes());
    dib.extend_from_slice(&2835i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..height as usize).rev() {
        let row = &rgba[y * row_size..(y + 1) * row_size];
        for pixel in row.chunks_exact(4) {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(dib)
}

#[repr(C)]
#[cfg(target_os = "windows")]
struct DropFilesHeader {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_test_platform_has_no_file_clipboard_contents() {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert!(super::read_clipboard_files().unwrap().is_none());
    }

    #[test]
    fn retry_delays_cover_busy_clipboard_windows() {
        let mut elapsed = std::time::Duration::ZERO;
        for attempt in 0..7 {
            elapsed += super::clipboard_retry_delay(attempt);
        }
        assert!(elapsed >= std::time::Duration::from_millis(500));
        assert!(elapsed < super::CLIPBOARD_READ_DEADLINE);
    }

    #[test]
    fn clipboard_error_is_typed_and_does_not_expose_content() {
        let error = super::ClipboardFileReadError::Busy {
            attempts: 4,
            elapsed_ms: 200,
        };
        assert_eq!(
            error.to_string(),
            "Windows clipboard remained busy after 4 attempts over 200ms"
        );
        assert!(!error.to_string().contains("clipboard contents"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "overwrites the current Windows clipboard; opt in with TAILSYNC_ALLOW_CLIPBOARD_OVERWRITE=1"]
    fn busy_file_clipboard_system_acceptance() {
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};
        use windows_sys::Win32::System::DataExchange::EmptyClipboard;

        assert_eq!(
            std::env::var("TAILSYNC_ALLOW_CLIPBOARD_OVERWRITE").as_deref(),
            Ok("1"),
            "explicit clipboard overwrite opt-in is required"
        );
        let root = std::env::var_os("TAILSYNC_CLIPBOARD_TEST_DIR")
            .expect("isolated synthetic clipboard test directory is required");
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).expect("create synthetic clipboard directory");
        let file = root.join(format!("synthetic-clipboard-{}.txt", std::process::id()));
        std::fs::write(&file, b"TailSync synthetic clipboard acceptance fixture")
            .expect("write synthetic clipboard fixture");

        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    if let Ok(_guard) = super::open_clipboard_with_retry() {
                        EmptyClipboard();
                    }
                }
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _fixture = Fixture(file.clone());
        super::write_clipboard_files(std::slice::from_ref(&file))
            .expect("publish synthetic CF_HDROP clipboard fixture");

        fn hold_clipboard(
            delay_ms: u64,
            root: &std::path::Path,
            label: &str,
        ) -> std::process::Child {
            let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../scripts/clipboard-owner-test.ps1");
            let ready = root.join(format!("clipboard-ready-{label}.txt"));
            let mut owner = std::process::Command::new("pwsh")
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(script)
                .arg("-HoldMilliseconds")
                .arg(delay_ms.to_string())
                .arg("-ReadyFile")
                .arg(&ready)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .expect("spawn isolated clipboard owner");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready.exists() {
                assert!(
                    Instant::now() < deadline,
                    "clipboard owner readiness timeout"
                );
                assert!(
                    owner.try_wait().expect("poll clipboard owner").is_none(),
                    "clipboard owner exited before readiness"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::remove_file(ready).expect("clear clipboard owner marker");
            owner
        }

        let iterations = std::env::var("TAILSYNC_CLIPBOARD_TEST_ITERATIONS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(20);
        assert!((1..=20).contains(&iterations));
        for delay_ms in [40_u64, 200, 500] {
            for iteration in 0..iterations {
                let mut owner = hold_clipboard(delay_ms, &root, &format!("{delay_ms}-{iteration}"));
                super::TEST_READ_ATTEMPTS.store(0, Ordering::Relaxed);
                let started = Instant::now();
                let files = super::read_clipboard_files()
                    .expect("busy clipboard should succeed within 750ms");
                let elapsed_ms = started.elapsed().as_millis();
                let attempts = super::TEST_READ_ATTEMPTS.load(Ordering::Relaxed);
                assert!(owner.wait().expect("wait for clipboard owner").success());
                assert_eq!(files.as_deref(), Some(std::slice::from_ref(&file)));
                assert!(attempts > 1, "busy clipboard must retry");
                println!(
                    "delay_ms={delay_ms} iteration={} attempts={attempts} elapsed_ms={elapsed_ms} outcome=file",
                    iteration + 1
                );
            }
        }

        let mut owner = hold_clipboard(900, &root, "timeout");
        super::TEST_READ_ATTEMPTS.store(0, Ordering::Relaxed);
        let started = Instant::now();
        let result = super::read_clipboard_files();
        let elapsed_ms = started.elapsed().as_millis();
        assert!(owner.wait().expect("wait for clipboard owner").success());
        let attempts = super::TEST_READ_ATTEMPTS.load(Ordering::Relaxed);
        assert!(matches!(
            result,
            Err(super::ClipboardFileReadError::Busy { .. })
        ));
        assert!(attempts > 1, "timeout must follow bounded retries");
        println!("delay_ms=900 attempts={attempts} elapsed_ms={elapsed_ms} outcome=typed_busy");
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
