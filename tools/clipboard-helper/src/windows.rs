use crate::format_model::{
    enumeration_outcome, resolve_name, EnumOutcome, FormatDiagnostic, FormatMetadata,
};
use std::io::{Read, Write};
use tailsync_protocol::{
    bitmap, native_archive::*, native_clipboard::*, native_policy, PackedImage,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::*, Memory::*},
    UI::WindowsAndMessaging::*,
};
const TEXT: u32 = 13;
const DIB: u32 = 8;
const DIBV5: u32 = 17;
const FILES: u32 = 15;
const SOURCE_FORMAT: &str = "org.tailsync.clipboard-source";
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn registered(s: &str) -> Result<u32, String> {
    let n = unsafe { RegisterClipboardFormatW(wide(s).as_ptr()) };
    if n == 0 {
        Err("Format registration failed".into())
    } else {
        Ok(n)
    }
}
struct Clipboard {
    window: HWND,
}
impl Clipboard {
    fn open() -> Result<Self, String> {
        unsafe {
            let window = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("TailSync clipboard helper").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if window.is_null() {
                return Err("Clipboard owner unavailable".into());
            }
            for _ in 0..25 {
                if OpenClipboard(window) != 0 {
                    return Ok(Self { window });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            DestroyWindow(window);
            Err("Clipboard busy".into())
        }
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
            DestroyWindow(self.window);
        }
    }
}
struct Buffer(HGLOBAL);
impl Buffer {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if h.is_null() {
                return Err("Clipboard allocation failed".into());
            }
            let p = GlobalLock(h);
            if p.is_null() {
                GlobalFree(h);
                return Err("Clipboard allocation lock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            GlobalUnlock(h);
            Ok(Self(h))
        }
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            GlobalFree(self.0);
        }
    }
}
fn read_format(id: u32) -> Result<Vec<u8>, FormatState> {
    unsafe {
        let h = GetClipboardData(id);
        if h.is_null() {
            return Err(FormatState::Failed);
        }
        let len = GlobalSize(h as HGLOBAL);
        if len == 0 {
            return Err(FormatState::Failed);
        }
        if len > NATIVE_LIMIT {
            return Err(FormatState::TooLarge);
        }
        let pointer = GlobalLock(h as HGLOBAL);
        if pointer.is_null() {
            return Err(FormatState::Failed);
        }
        let bytes = std::slice::from_raw_parts(pointer as *const u8, len).to_vec();
        GlobalUnlock(h as HGLOBAL);
        Ok(bytes)
    }
}
fn png_from_packed(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = PackedImage::try_from(bytes).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(image.rgba)
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}
fn packed_from_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    tailsync_protocol::composite::png_rgba_size(bytes).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        png::Limits {
            bytes: 64 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    if reader.info().animation_control.is_some() {
        return Err("Animated clipboard PNG unsupported".into());
    }
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= 32 * 1024 * 1024)
        .ok_or("PNG size exceeds limit")?;
    let mut decoded = vec![0; size];
    let info = reader.next_frame(&mut decoded).map_err(|e| e.to_string())?;
    reader.finish().map_err(|e| e.to_string())?;
    let stride = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return Err("PNG color unsupported".into()),
    };
    let mut rgba = vec![];
    for p in decoded[..info.buffer_size()].chunks_exact(stride) {
        match stride {
            4 => rgba.extend_from_slice(p),
            3 => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
            2 => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            _ => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
        }
    }
    tailsync_protocol::pack_rgba_image(info.width, info.height, &rgba).map_err(|e| e.to_string())
}
fn registered_name(id: u32) -> Option<String> {
    let mut buffer = [0u16; 512];
    let length = unsafe { GetClipboardFormatNameW(id, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return None;
    }
    let length = (length as usize).min(buffer.len());
    let name = String::from_utf16_lossy(&buffer[..length]);
    if name.is_empty() || name.chars().any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}') {
        None
    } else {
        Some(name)
    }
}

/// Presence or fixed DWORD only. Any read failure is reported as an invalid
/// value, never as permission.
fn control_payload(id: u32) -> Result<Option<Vec<u8>>, String> {
    if unsafe { IsClipboardFormatAvailable(id) } == 0 {
        return Ok(None);
    }
    match read_format(id) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) => Ok(Some(Vec::new())),
    }
}

fn privacy_decision() -> Result<native_policy::WindowsPrivacy, String> {
    let exclude = registered(native_policy::WINDOWS_EXCLUDE_MONITOR)?;
    if unsafe { IsClipboardFormatAvailable(exclude) } != 0 {
        return Ok(native_policy::WindowsPrivacy::Suppress);
    }
    let history = control_payload(registered(native_policy::WINDOWS_CAN_INCLUDE_HISTORY)?)?;
    let cloud = control_payload(registered(native_policy::WINDOWS_CAN_UPLOAD_CLOUD)?)?;
    Ok(native_policy::windows_privacy_decision(
        false,
        history.as_deref(),
        cloud.as_deref(),
    ))
}

fn metadata_only_archive(before: i64, after: i64, state: SnapshotState) -> NativeArchive {
    NativeArchive {
        platform: 2,
        enumeration_complete: false,
        original: NativeSnapshot {
            before,
            after,
            items: 0,
            state,
            formats: vec![],
        },
        derived: vec![],
    }
}

fn archive_from_parts(
    before: i64,
    after: i64,
    formats: Vec<NativeFormat>,
    mut derived: Vec<DerivedFormat>,
) -> NativeArchive {
    let state = if before == 0 || after == 0 {
        SnapshotState::Failed
    } else if before != after {
        SnapshotState::Unstable
    } else if formats.is_empty() {
        SnapshotState::Empty
    } else {
        SnapshotState::Stable
    };
    if state != SnapshotState::Stable {
        derived.clear();
    }
    NativeArchive {
        platform: 2,
        enumeration_complete: false,
        original: NativeSnapshot {
            before,
            after,
            items: if formats.is_empty() { 0 } else { 1 },
            state,
            formats,
        },
        derived,
    }
}

/// CNA1 platform 2 read. Originals stay originals; the PNG produced from a DIB
/// is an explicit converter-derived representation and is never placed in
/// `original`.
fn read_archive() -> Result<NativeArchive, String> {
    let clipboard = match Clipboard::open() {
        Ok(clipboard) => clipboard,
        Err(_) => return Ok(metadata_only_archive(0, 0, SnapshotState::Busy)),
    };
    let before = unsafe { GetClipboardSequenceNumber() } as i64;
    if before == 0 {
        drop(clipboard);
        return Ok(metadata_only_archive(0, 0, SnapshotState::Failed));
    }
    match native_policy::windows_read_plan(privacy_decision()?) {
        native_policy::WindowsReadPlan::ReadBody => {}
        native_policy::WindowsReadPlan::Suppress => {
            let after = unsafe { GetClipboardSequenceNumber() } as i64;
            drop(clipboard);
            let state = if after == before {
                SnapshotState::Suppressed
            } else if after == 0 {
                SnapshotState::Failed
            } else {
                SnapshotState::Unstable
            };
            return Ok(metadata_only_archive(before, after, state));
        }
        native_policy::WindowsReadPlan::Failed => {
            let after = unsafe { GetClipboardSequenceNumber() } as i64;
            drop(clipboard);
            return Ok(metadata_only_archive(before, after, SnapshotState::Failed));
        }
    }

    let known = [
        (TEXT, "CF_UNICODETEXT"),
        (registered("PNG")?, "PNG"),
        (DIBV5, "CF_DIBV5"),
        (DIB, "CF_DIB"),
        (FILES, "CF_HDROP"),
        (registered("HTML Format")?, "HTML Format"),
        (registered("Rich Text Format")?, "Rich Text Format"),
        (registered(SOURCE_FORMAT)?, SOURCE_FORMAT),
    ];
    let mut formats: Vec<NativeFormat> = vec![];
    let mut used = 32usize;
    for (id, name) in known {
        if unsafe { IsClipboardFormatAvailable(id) } == 0 {
            continue;
        }
        if name == "CF_DIB"
            && formats.iter().any(|f| {
                f.name == "CF_DIBV5"
                    && f.state == FormatState::Captured
                    && bitmap::decode_dib(&f.bytes).is_ok()
            })
        {
            continue;
        }
        let (state, bytes) = match read_format(id) {
            Ok(mut bytes) => {
                if name == "CF_UNICODETEXT" {
                    if let Some(n) = bytes.chunks_exact(2).position(|pair| pair == [0, 0]) {
                        bytes.truncate((n + 1) * 2);
                    }
                } else if matches!(name, "HTML Format" | "Rich Text Format") {
                    while bytes.last() == Some(&0) {
                        bytes.pop();
                    }
                }
                if used.saturating_add(bytes.len() + name.len() + 12) > NATIVE_LIMIT {
                    (FormatState::TooLarge, vec![])
                } else {
                    used += bytes.len() + name.len() + 12;
                    (FormatState::Captured, bytes)
                }
            }
            Err(state) => (state, vec![]),
        };
        formats.push(NativeFormat {
            item: 0,
            name: name.into(),
            state,
            bytes,
        });
    }
    let after = unsafe { GetClipboardSequenceNumber() } as i64;
    drop(clipboard);

    let mut derived = vec![];
    if before == after && !formats.is_empty() && !formats.iter().any(|f| f.name == "PNG") {
        if let Some(dib) = formats
            .iter()
            .find(|f| f.name == "CF_DIBV5" && f.state == FormatState::Captured)
            .or_else(|| {
                formats
                    .iter()
                    .find(|f| f.name == "CF_DIB" && f.state == FormatState::Captured)
            })
        {
            let png = bitmap::decode_dib(&dib.bytes)
                .map_err(|e| e.to_string())
                .and_then(|packed| png_from_packed(&packed));
            if let Ok(png) = png {
                let overhead =
                    12 + "PNG".len() + dib.name.len() + CONVERTER_WINDOWS_DIB_TO_PNG.len();
                if used.saturating_add(overhead).saturating_add(png.len()) <= NATIVE_LIMIT {
                    derived.push(DerivedFormat {
                        item: 0,
                        name: "PNG".into(),
                        source_name: dib.name.clone(),
                        converter: CONVERTER_WINDOWS_DIB_TO_PNG.into(),
                        bytes: png,
                    });
                }
            }
        }
    }
    Ok(archive_from_parts(before, after, formats, derived))
}

/// Metadata-only diagnostic. It never calls GetClipboardData, GlobalSize or a
/// payload probe, and it does not allocate storage proportional to body bytes.
fn dump_formats() -> Result<Vec<u8>, String> {
    let clipboard = match Clipboard::open() {
        Ok(clipboard) => clipboard,
        Err(_) => {
            return Ok(FormatDiagnostic::collect(0, 0, false, Vec::new())
                .render()
                .into_bytes())
        }
    };
    let before = unsafe { GetClipboardSequenceNumber() } as i64;
    let mut ids = vec![];
    let mut previous = 0u32;
    let mut complete = true;
    loop {
        unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
        let id = unsafe { EnumClipboardFormats(previous) };
        let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if id == 0 {
            if matches!(enumeration_outcome(id, error), EnumOutcome::Error) {
                complete = false;
            }
            break;
        }
        ids.push(id);
        previous = id;
        if ids.len() > crate::format_model::FORMAT_DIAGNOSTIC_LIMIT {
            break;
        }
    }
    let after = unsafe { GetClipboardSequenceNumber() } as i64;
    drop(clipboard);
    let entries = ids.into_iter().map(|id| {
        let (name, name_kind, capture_class) = resolve_name(id, registered_name);
        FormatMetadata::observed(id, name, name_kind, capture_class)
    });
    Ok(FormatDiagnostic::collect(before, after, complete, entries)
        .render()
        .into_bytes())
}
fn snapshot() -> Result<NativeSnapshot, String> {
    let clipboard = match Clipboard::open() {
        Ok(c) => c,
        Err(_) => {
            return Ok(NativeSnapshot {
                before: 0,
                after: 0,
                items: 0,
                state: SnapshotState::Busy,
                formats: vec![],
            })
        }
    };
    let before = unsafe { GetClipboardSequenceNumber() } as i64;
    let known = [
        (TEXT, "CF_UNICODETEXT"),
        (registered("PNG")?, "PNG"),
        (DIBV5, "CF_DIBV5"),
        (DIB, "CF_DIB"),
        (FILES, "CF_HDROP"),
        (registered("HTML Format")?, "HTML Format"),
        (registered("Rich Text Format")?, "Rich Text Format"),
        (
            registered("org.tailsync.clipboard-source")?,
            "org.tailsync.clipboard-source",
        ),
    ];
    let mut formats: Vec<NativeFormat> = vec![];
    let mut used = 32usize;
    for (id, name) in known {
        if unsafe { IsClipboardFormatAvailable(id) } == 0 {
            continue;
        }
        if name == "CF_DIB"
            && formats.iter().any(|f| {
                f.name == "CF_DIBV5"
                    && f.state == FormatState::Captured
                    && bitmap::decode_dib(&f.bytes).is_ok()
            })
        {
            continue;
        }
        let (state, bytes) = match read_format(id) {
            Ok(mut b) => {
                if name == "CF_UNICODETEXT" {
                    if let Some(n) = b.chunks_exact(2).position(|p| p == [0, 0]) {
                        b.truncate((n + 1) * 2);
                    }
                } else if matches!(name, "HTML Format" | "Rich Text Format") {
                    while b.last() == Some(&0) {
                        b.pop();
                    }
                }
                if used.saturating_add(b.len() + name.len() + 12) > NATIVE_LIMIT {
                    (FormatState::TooLarge, vec![])
                } else {
                    used += b.len() + name.len() + 12;
                    (FormatState::Captured, b)
                }
            }
            Err(s) => (s, vec![]),
        };
        formats.push(NativeFormat {
            item: 0,
            name: name.into(),
            state,
            bytes,
        });
    }
    let after = unsafe { GetClipboardSequenceNumber() } as i64;
    drop(clipboard); // Conversion must not hold the system clipboard open.
    if !formats.iter().any(|f| f.name == "PNG") {
        if let Some(dib) = formats
            .iter()
            .find(|f| f.name == "CF_DIBV5")
            .or_else(|| formats.iter().find(|f| f.name == "CF_DIB"))
        {
            let png = if dib.state == FormatState::Captured {
                bitmap::decode_dib(&dib.bytes)
                    .map_err(|e| e.to_string())
                    .and_then(|b| png_from_packed(&b))
            } else {
                Err("Bitmap read failed".into())
            };
            let (state, bytes) = match png {
                Ok(b) if used + b.len() + 64 <= NATIVE_LIMIT => (FormatState::Captured, b),
                Ok(_) => (FormatState::TooLarge, vec![]),
                Err(_) => (FormatState::Failed, vec![]),
            };
            formats.push(NativeFormat {
                item: 0,
                name: "PNG".into(),
                state,
                bytes,
            });
        }
    }
    let state = if before == 0 || after == 0 {
        SnapshotState::Failed
    } else if before != after {
        SnapshotState::Unstable
    } else if formats.is_empty() {
        SnapshotState::Empty
    } else {
        SnapshotState::Stable
    };
    Ok(NativeSnapshot {
        before,
        after,
        items: if formats.is_empty() { 0 } else { 1 },
        state,
        formats,
    })
}
fn publish(input: &[u8]) -> Result<PublishReceipt, String> {
    if input.len() < 24 {
        return Err("Invalid clipboard input".into());
    }
    let token: [u8; 16] = input[..16].try_into().map_err(|_| "Invalid token")?;
    let expected = i64::from_be_bytes(input[16..24].try_into().map_err(|_| "Invalid revision")?);
    let snapshot = NativeSnapshot::decode(&input[24..]).map_err(|e| e.to_string())?;
    if snapshot.state != SnapshotState::Stable || snapshot.items != 1 || snapshot.formats.is_empty()
    {
        return Err("Unsupported clipboard shape".into());
    }
    let mut prepared = vec![];
    for f in &snapshot.formats {
        if f.state != FormatState::Captured || f.item != 0 {
            return Err("Incomplete publish input".into());
        }
        let id = match f.name.as_str() {
            "CF_UNICODETEXT" => TEXT,
            "HTML Format" => registered("HTML Format")?,
            "Rich Text Format" => registered("Rich Text Format")?,
            "PNG" => registered("PNG")?,
            "CF_DIB" => DIB,
            "CF_DIBV5" => DIBV5,
            _ => return Err("Unsupported publish format".into()),
        };
        if matches!(id, DIB | DIBV5) {
            bitmap::decode_dib(&f.bytes).map_err(|e| e.to_string())?;
        }
        if id == TEXT
            && (f.bytes.len() % 2 != 0
                || !f.bytes.ends_with(&[0, 0])
                || f.bytes.len() > 2 * 1024 * 1024 + 2)
        {
            return Err("Invalid Unicode text".into());
        }
        let mut bytes = f.bytes.clone();
        if matches!(f.name.as_str(), "HTML Format" | "Rich Text Format") && bytes.last() != Some(&0)
        {
            bytes.push(0);
        }
        prepared.push((id, Buffer::new(&bytes)?));
        // R5-07: publishing PNG plus CF_DIBV5 and deliberately **not** CF_DIB is the
        // documented-correct behaviour, not a gap. Microsoft's Clipboard Formats page
        // lists CF_DIBV5 -> CF_DIB under "Synthesized Clipboard Formats" and states:
        // "If the system provides an automatic type conversion for a particular
        // clipboard format, there is no advantage to placing the conversion format(s)
        // on the clipboard." It also says that when copying bitmaps it is best to
        // place "the CF_DIB or CF_DIBV5 format" — either satisfies consumers, because
        // the system renders the other on request.
        // https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats
        //
        // Consequence for the acceptance review's H-10: the *absence* of CF_DIB is not
        // evidence that a target application cannot paste, so it cannot be used to
        // explain a paste failure. Adding a compatibility DIB needs an observed
        // failure in a named target application (still UNKNOWN; plan APP-07 / R7).
        if f.name == "PNG" && !snapshot.formats.iter().any(|f| f.name == "CF_DIBV5") {
            let packed = packed_from_png(&f.bytes)?;
            let dib = bitmap::encode_dib_v5(
                PackedImage::try_from(packed.as_slice()).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            prepared.push((DIBV5, Buffer::new(&dib)?));
        }
    }
    prepared.push((
        registered("org.tailsync.clipboard-source")?,
        Buffer::new(&token)?,
    ));
    let clipboard = match Clipboard::open() {
        Ok(c) => c,
        Err(_) => {
            return Ok(PublishReceipt {
                state: PublishState::Failed,
                revision: 0,
                token,
            })
        }
    };
    let before = unsafe { GetClipboardSequenceNumber() } as i64;
    if before == 0 || expected != -1 && expected != before {
        return Ok(PublishReceipt {
            state: PublishState::Superseded,
            revision: before,
            token,
        });
    }
    if unsafe { EmptyClipboard() } == 0 {
        return Ok(PublishReceipt {
            state: PublishState::Failed,
            revision: before,
            token,
        });
    }
    let mut state = PublishState::Published;
    for (id, buffer) in prepared {
        if unsafe { SetClipboardData(id, buffer.0 as HANDLE) }.is_null() {
            state = PublishState::Partial;
            break;
        }
        std::mem::forget(buffer);
    }
    // CloseClipboard finalizes the write and Windows may synthesize formats or
    // advance the sequence again when the short-lived owner is destroyed.
    // The receipt must identify the state a separate monitor can actually read.
    drop(clipboard);
    let revision = unsafe { GetClipboardSequenceNumber() } as i64;
    if revision == 0 {
        state = PublishState::Partial;
    }
    Ok(PublishReceipt {
        state,
        revision,
        token,
    })
}
pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = match args.as_slice() {
        // Startup probes must advertise the wire protocol without opening or
        // reading the clipboard.
        [s] if s == "--self-check" => b"CWH1 protocol=3 analyze=1\n".to_vec(),
        [s] if s == "--dump-formats" => dump_formats()?,
        [s] if s == "--read-archive" => read_archive()?.encode().map_err(|e| e.to_string())?,
        [s] if s == "--read-snapshot" => snapshot()?.encode().map_err(|e| e.to_string())?,
        [s] if s == "--write-snapshot" => {
            let mut input = vec![];
            std::io::stdin()
                .take(NATIVE_LIMIT as u64 + 25)
                .read_to_end(&mut input)
                .map_err(|e| e.to_string())?;
            if input.len() > NATIVE_LIMIT + 24 {
                return Err("Clipboard input exceeds limit".into());
            }
            publish(&input)?.encode()
        }
        _ => return Err("Invalid helper command".into()),
    };
    std::io::stdout().write_all(&out).map_err(|e| e.to_string())
}
