#[cfg(target_os = "macos")]
pub struct ClipboardChangeDetector {
    last_change_count: isize,
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

    pub fn changed(&mut self) -> bool {
        true
    }
}
