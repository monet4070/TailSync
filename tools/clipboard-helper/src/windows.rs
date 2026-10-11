use std::io::{Read, Write};
use tailsync_protocol::{bitmap, native_clipboard::*, PackedImage};
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::*, Memory::*},
    UI::WindowsAndMessaging::*,
};
const TEXT: u32 = 13;
const DIB: u32 = 8;
const DIBV5: u32 = 17;
const FILES: u32 = 15;
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
        (DIBV5, "CF_DIBV5"),
        (DIB, "CF_DIB"),
        (FILES, "CF_HDROP"),
        (registered("HTML Format")?, "HTML Format"),
        (registered("Rich Text Format")?, "Rich Text Format"),
        (registered("PNG")?, "PNG"),
        (
            registered("org.tailsync.clipboard-source")?,
            "org.tailsync.clipboard-source",
        ),
    ];
    let mut formats = vec![];
    let mut used = 32usize;
    for (id, name) in known {
        if unsafe { IsClipboardFormatAvailable(id) } == 0 {
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
    let _clipboard = match Clipboard::open() {
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
