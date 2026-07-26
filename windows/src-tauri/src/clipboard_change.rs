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

    pub fn reports_native_change_events(&self) -> bool {
        true
    }
}

#[cfg(target_os = "windows")]
pub struct ClipboardChangeDetector {
    last_sequence_number: u32,
}

#[cfg(target_os = "windows")]
impl ClipboardChangeDetector {
    pub fn new() -> Self {
        Self {
            last_sequence_number: unsafe {
                windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber()
            },
        }
    }

    pub fn poll_interval_ms(&self) -> u64 {
        50
    }

    pub fn changed(&mut self) -> bool {
        let current =
            unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() };
        sequence_number_changed(&mut self.last_sequence_number, current)
    }

    pub fn reports_native_change_events(&self) -> bool {
        true
    }
}

#[cfg(target_os = "windows")]
fn sequence_number_changed(last: &mut u32, current: u32) -> bool {
    if current == 0 || current == *last {
        return false;
    }
    *last = current;
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct ClipboardChangeDetector;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

    pub fn reports_native_change_events(&self) -> bool {
        false
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::sequence_number_changed;

    #[test]
    fn unchanged_sequence_is_not_a_clipboard_event() {
        let mut last = 42;
        assert!(!sequence_number_changed(&mut last, 42));
        assert_eq!(last, 42);
    }

    #[test]
    fn new_sequence_is_consumed_once() {
        let mut last = 42;
        assert!(sequence_number_changed(&mut last, 43));
        assert_eq!(last, 43);
        assert!(!sequence_number_changed(&mut last, 43));
    }

    #[test]
    fn unavailable_sequence_does_not_create_a_false_event() {
        let mut last = 42;
        assert!(!sequence_number_changed(&mut last, 0));
        assert_eq!(last, 42);
    }
}
