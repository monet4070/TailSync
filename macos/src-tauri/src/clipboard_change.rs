#[cfg(target_os = "macos")]
pub struct ClipboardChangeDetector {
    last_change_count: isize,
}

#[cfg(target_os = "macos")]
struct WriteReceipt {
    change_count: isize,
    hash: String,
}

#[cfg(target_os = "macos")]
static WRITE_RECEIPT: std::sync::OnceLock<std::sync::Mutex<Option<WriteReceipt>>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
fn write_receipt() -> &'static std::sync::Mutex<Option<WriteReceipt>> {
    WRITE_RECEIPT.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(target_os = "macos")]
fn clipboard_change_count() -> isize {
    use objc2_app_kit::NSPasteboard;
    NSPasteboard::generalPasteboard().changeCount()
}

#[cfg(target_os = "macos")]
pub fn record_text_write_receipt(hash: &str) {
    *write_receipt()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(WriteReceipt {
        change_count: clipboard_change_count(),
        hash: hash.to_string(),
    });
}

#[cfg(target_os = "macos")]
pub fn consume_text_write_receipt(hash: &str) -> bool {
    let change_count = clipboard_change_count();
    let mut receipt = write_receipt()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let matches = receipt
        .as_ref()
        .is_some_and(|value| value.change_count == change_count && value.hash == hash);
    let stale = receipt
        .as_ref()
        .is_some_and(|value| value.change_count != change_count);
    if matches || stale {
        receipt.take();
    }
    matches
}

#[cfg(not(target_os = "macos"))]
pub fn record_text_write_receipt(_hash: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn consume_text_write_receipt(_hash: &str) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn record_image_write_receipt(hash: &str) {
    record_text_write_receipt(hash);
}

#[cfg(target_os = "macos")]
pub fn consume_image_write_receipt(hash: &str) -> bool {
    consume_text_write_receipt(hash)
}

#[cfg(not(target_os = "macos"))]
pub fn record_image_write_receipt(_hash: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn consume_image_write_receipt(_hash: &str) -> bool {
    false
}

#[cfg(target_os = "macos")]
impl ClipboardChangeDetector {
    pub fn new() -> Self {
        use objc2_app_kit::NSPasteboard;

        Self {
            last_change_count: NSPasteboard::generalPasteboard().changeCount(),
        }
    }

    pub fn poll_interval_ms(&self) -> u64 {
        50
    }

    pub fn idle_poll_interval_ms(&self) -> u64 {
        250
    }

    pub fn changed(&mut self) -> bool {
        use objc2_app_kit::NSPasteboard;

        let current = NSPasteboard::generalPasteboard().changeCount();
        if current == self.last_change_count {
            return false;
        }
        self.last_change_count = current;
        true
    }
}

#[cfg(not(target_os = "macos"))]
pub struct ClipboardChangeDetector;

#[cfg(not(target_os = "macos"))]
impl ClipboardChangeDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn poll_interval_ms(&self) -> u64 {
        200
    }

    pub fn idle_poll_interval_ms(&self) -> u64 {
        200
    }

    pub fn changed(&mut self) -> bool {
        true
    }
}
