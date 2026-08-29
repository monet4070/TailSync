use super::*;

/// Get current clipboard version (for polling-based refresh)
#[command]
pub async fn get_version() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": crate::api::get_clipboard_version()
    }))
}

#[command]
pub async fn get_sync_warning() -> Result<Option<tailsync_core::sync_warning::SyncWarning>, String>
{
    Ok(tailsync_core::sync_warning::take())
}

/// Convert RGBA to CF_DIB clipboard format (BITMAPINFOHEADER + bottom-up BGRA pixels).
/// No file header — this is what Windows stores in the clipboard as CF_DIB.
pub(super) fn rgba_to_dib(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let w = w as i32;
    let h = h as i32;
    let row_size = w * 4; // 32 bpp, naturally 4-byte aligned
    let pixel_data_size = (row_size * h) as u32;
    let dib_size = 40 + pixel_data_size as usize;

    let mut dib = Vec::with_capacity(dib_size);

    // BITMAPINFOHEADER (40 bytes)
    dib.extend_from_slice(&(40u32).to_le_bytes());
    dib.extend_from_slice(&w.to_le_bytes());
    dib.extend_from_slice(&h.to_le_bytes());
    dib.extend_from_slice(&(1u16).to_le_bytes()); // planes
    dib.extend_from_slice(&(32u16).to_le_bytes()); // bpp = 32
    dib.extend_from_slice(&(0u32).to_le_bytes()); // BI_RGB (no compression)
    dib.extend_from_slice(&pixel_data_size.to_le_bytes());
    dib.extend_from_slice(&(2835i32).to_le_bytes()); // 72 DPI
    dib.extend_from_slice(&(2835i32).to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());

    // Pixels: bottom-up, RGBA → BGRA
    for y in (0..h).rev() {
        let src_start = (y * w * 4) as usize;
        let src_end = src_start + (w * 4) as usize;
        if src_end > rgba.len() {
            break;
        }
        let row = &rgba[src_start..src_end];
        for x in 0..w as usize {
            dib.push(row[x * 4 + 2]); // B
            dib.push(row[x * 4 + 1]); // G
            dib.push(row[x * 4]); // R
            dib.push(row[x * 4 + 3]); // A
        }
    }

    dib
}

/// Set CF_DIB data on the Windows clipboard (raw Win32, no file involved).
#[cfg(target_os = "windows")]
pub(super) fn set_clipboard_dib(dib: &[u8]) {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GHND, dib.len());
        if !h.is_null() {
            let ptr = GlobalLock(h) as *mut u8;
            std::ptr::copy_nonoverlapping(dib.as_ptr(), ptr, dib.len());
            GlobalUnlock(h);
            SetClipboardData(8, h); // CF_DIB = 8
        }
        CloseClipboard();
    }
}
