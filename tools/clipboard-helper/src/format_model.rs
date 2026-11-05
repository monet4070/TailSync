//! Pure Win32 clipboard metadata model.
//!
//! This module deliberately has no Win32 imports: it can be exercised on any
//! host, and it has no operation that could read a clipboard payload. The
//! Windows adapter is responsible only for enumerating format identifiers and
//! resolving names; all limits, classification and JSON rendering live here.

use std::fmt::Write as _;

pub const DIAGNOSTIC_VERSION: u32 = 1;
pub const FORMAT_DIAGNOSTIC_LIMIT: usize = 256;
pub const OUTPUT_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumOutcome {
    End,
    Error,
}

/// `EnumClipboardFormats` returns zero for both a normal end and a failure.
/// The caller must consult `GetLastError` to tell them apart (MS1).
pub fn enumeration_outcome(returned: u32, last_error: u32) -> EnumOutcome {
    if returned == 0 && last_error == 0 {
        EnumOutcome::End
    } else {
        EnumOutcome::Error
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Standard,
    Registered,
    PrivateUnnamed,
    LookupFailed,
}

impl NameKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Registered => "registered",
            Self::PrivateUnnamed => "private-unnamed",
            Self::LookupFailed => "lookup-failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureClass {
    KnownHGlobal,
    KnownGdiHandle,
    DesktopInternal,
    UnknownRegistered,
    PrivateUnnamed,
}

impl CaptureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownHGlobal => "known-hglobal",
            Self::KnownGdiHandle => "known-gdi-handle",
            Self::DesktopInternal => "desktop-internal",
            Self::UnknownRegistered => "unknown-registered",
            Self::PrivateUnnamed => "private-unnamed",
        }
    }
}

/// Fixed standard-format identity. `GetClipboardFormatNameW` is only valid for
/// registered formats; treating its zero return as "unnamed" for standard IDs
/// would invent a non-existent format (MS8).
pub fn standard_format_name(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "CF_TEXT",
        2 => "CF_BITMAP",
        3 => "CF_METAFILEPICT",
        4 => "CF_SYLK",
        5 => "CF_DIF",
        6 => "CF_TIFF",
        7 => "CF_OEMTEXT",
        8 => "CF_DIB",
        9 => "CF_PALETTE",
        10 => "CF_PENDATA",
        11 => "CF_RIFF",
        12 => "CF_WAVE",
        13 => "CF_UNICODETEXT",
        14 => "CF_ENHMETAFILE",
        15 => "CF_HDROP",
        16 => "CF_LOCALE",
        17 => "CF_DIBV5",
        0x0080 => "CF_OWNERDISPLAY",
        0x0081 => "CF_DSPTEXT",
        0x0082 => "CF_DSPBITMAP",
        0x0083 => "CF_DSPMETAFILEPICT",
        0x008E => "CF_DSPENHMETAFILE",
        _ => return None,
    })
}

fn capture_class_for_standard(id: u32) -> CaptureClass {
    match id {
        // Handle-valued or desktop-owned standard formats must not be read as
        // HGLOBAL. They are recorded as present and left unread.
        2 | 3 | 9 | 14 => CaptureClass::KnownGdiHandle,
        0x0080..=0x008E => CaptureClass::DesktopInternal,
        0x0200..=0x02FF => CaptureClass::DesktopInternal,
        0x0300..=0x03FF => CaptureClass::DesktopInternal,
        _ => CaptureClass::KnownHGlobal,
    }
}

/// A diagnostic display identity for an unnamed private numeric format.
/// This is not a stable cross-process name; it only makes the observation
/// traceable inside this one dump.
pub fn private_display_name(id: u32) -> String {
    format!("private-format-0x{id:04X}")
}

/// Resolve a format name without asking the OS for standard formats.
/// The lookup closure is only invoked for registered-format identifiers.
pub fn resolve_name<F>(id: u32, lookup: F) -> (String, NameKind, CaptureClass)
where
    F: FnOnce(u32) -> Option<String>,
{
    if let Some(name) = standard_format_name(id) {
        return (
            name.to_string(),
            NameKind::Standard,
            capture_class_for_standard(id),
        );
    }
    if id >= 0xC000 {
        return match lookup(id) {
            Some(name) if !name.is_empty() => {
                (name, NameKind::Registered, CaptureClass::UnknownRegistered)
            }
            _ => (
                private_display_name(id),
                NameKind::LookupFailed,
                CaptureClass::UnknownRegistered,
            ),
        };
    }
    (
        private_display_name(id),
        NameKind::PrivateUnnamed,
        CaptureClass::PrivateUnnamed,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatMetadata {
    pub id: u32,
    pub name: String,
    pub name_kind: NameKind,
    pub capture_class: CaptureClass,
    pub read_attempted: bool,
    pub byte_length: Option<u32>,
}

impl FormatMetadata {
    /// Metadata-only constructor. There is deliberately no byte parameter.
    pub fn observed(
        id: u32,
        name: String,
        name_kind: NameKind,
        capture_class: CaptureClass,
    ) -> Self {
        Self {
            id,
            name,
            name_kind,
            capture_class,
            read_attempted: false,
            byte_length: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDiagnostic {
    pub before: i64,
    pub after: i64,
    pub stable: bool,
    pub win32_enumeration_complete: bool,
    pub truncated: bool,
    pub item_count: u16,
    pub formats: Vec<FormatMetadata>,
}

impl FormatDiagnostic {
    /// Consume at most one entry beyond the diagnostic limit. The extra entry
    /// only proves truncation; the remaining iterator is not materialised.
    pub fn collect<I>(before: i64, after: i64, complete: bool, entries: I) -> Self
    where
        I: IntoIterator<Item = FormatMetadata>,
    {
        let mut iter = entries.into_iter();
        let formats: Vec<FormatMetadata> = iter.by_ref().take(FORMAT_DIAGNOSTIC_LIMIT).collect();
        let truncated = iter.next().is_some();
        let item_count = if formats.is_empty() { 0 } else { 1 };
        let mut diagnostic = Self {
            before,
            after,
            stable: before == after && before != 0,
            win32_enumeration_complete: complete && !truncated,
            truncated,
            item_count,
            formats,
        };
        diagnostic.fit_output_budget();
        diagnostic
    }

    fn fit_output_budget(&mut self) {
        while self.render().len() > OUTPUT_LIMIT && !self.formats.is_empty() {
            self.formats.pop();
            self.truncated = true;
            self.win32_enumeration_complete = false;
        }
        if self.formats.is_empty() {
            self.item_count = 0;
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::with_capacity(512 + self.formats.len() * 160);
        let _ = write!(
            out,
            "{{\"diagnosticVersion\":{},\"scope\":\"win32-visible-formats\",\"before\":{},\"after\":{},\"stable\":{},\"win32EnumerationComplete\":{},\"truncated\":{},\"itemCount\":{},\"formats\":[",
            DIAGNOSTIC_VERSION,
            self.before,
            self.after,
            self.stable,
            self.win32_enumeration_complete,
            self.truncated,
            self.item_count
        );
        for (index, f) in self.formats.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"id\":{},\"name\":\"{}\",\"nameKind\":\"{}\",\"available\":true,\"readAttempted\":{},\"byteLength\":{},\"captureClass\":\"{}\"}}",
                f.id,
                escape_json(&f.name),
                f.name_kind.as_str(),
                f.read_attempted,
                match f.byte_length {
                    Some(value) => value.to_string(),
                    None => "null".to_string(),
                },
                f.capture_class.as_str()
            );
        }
        out.push_str("]}");
        out
    }
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(escaped, "\\u{:04X}", c as u32);
            }
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(id: u32, name: &str) -> FormatMetadata {
        let capture_class = match standard_format_name(id) {
            Some(_) => capture_class_for_standard(id),
            None => CaptureClass::UnknownRegistered,
        };
        FormatMetadata::observed(id, name.to_string(), NameKind::Registered, capture_class)
    }

    #[test]
    fn wcap_metadata_dump_never_requests_payload() {
        let diagnostic = FormatDiagnostic::collect(
            7,
            7,
            true,
            vec![observed(13, "CF_UNICODETEXT"), observed(8, "CF_DIB")],
        );
        let json = diagnostic.render();
        assert!(json.contains("\"readAttempted\":false"));
        assert!(json.contains("\"byteLength\":null"));
        assert!(json.contains("\"itemCount\":1"));
    }

    #[test]
    fn wcap_standard_name_does_not_require_registered_name_lookup() {
        let mut lookups = 0usize;
        let (name, kind, class) = resolve_name(13, |_| {
            lookups += 1;
            Some("must-not-be-used".to_string())
        });
        assert_eq!(name, "CF_UNICODETEXT");
        assert_eq!(kind, NameKind::Standard);
        assert_eq!(class, CaptureClass::KnownHGlobal);
        assert_eq!(lookups, 0);

        let (_, kind, _) = resolve_name(0xC123, |id| {
            lookups += 1;
            Some(format!("registered-{id}"))
        });
        assert_eq!(kind, NameKind::Registered);
        assert_eq!(lookups, 1);
    }

    #[test]
    fn wcap_enum_error_is_not_successful_end() {
        assert_eq!(enumeration_outcome(0, 0), EnumOutcome::End);
        assert_eq!(enumeration_outcome(0, 5), EnumOutcome::Error);
        assert_ne!(enumeration_outcome(0, 5), EnumOutcome::End);
    }

    #[test]
    fn wcap_duplicate_or_truncated_names_do_not_alias() {
        // Two distinct format IDs can expose the same registered name after a
        // short/truncated lookup. They stay separate observations.
        let diagnostic = FormatDiagnostic::collect(
            3,
            3,
            true,
            vec![observed(0xC001, "same-name"), observed(0xC002, "same-name")],
        );
        assert_eq!(diagnostic.formats.len(), 2);
        assert_eq!(diagnostic.item_count, 1);
        let json = diagnostic.render();
        assert!(json.contains("\"id\":49153"));
        assert!(json.contains("\"id\":49154"));
    }

    #[test]
    fn wcap_many_formats_report_truncation_without_unbounded_allocation() {
        let entries = (0..300u32).map(|index| observed(0xC000 + index, "registered"));
        let diagnostic = FormatDiagnostic::collect(5, 5, true, entries);
        assert_eq!(diagnostic.formats.len(), FORMAT_DIAGNOSTIC_LIMIT);
        assert!(diagnostic.truncated);
        assert!(!diagnostic.win32_enumeration_complete);
        assert_eq!(diagnostic.item_count, 1);
    }

    #[test]
    fn wcap_multiple_formats_do_not_create_multiple_items() {
        let diagnostic = FormatDiagnostic::collect(
            9,
            9,
            true,
            vec![
                observed(13, "CF_UNICODETEXT"),
                observed(8, "CF_DIB"),
                observed(0xC001, "HTML Format"),
            ],
        );
        assert_eq!(diagnostic.formats.len(), 3);
        assert_eq!(diagnostic.item_count, 1);
    }

    #[test]
    fn wcap_rendered_dump_stays_within_output_budget() {
        let long = "n".repeat(4096);
        let entries =
            (0..FORMAT_DIAGNOSTIC_LIMIT as u32).map(|index| observed(0xC000 + index, &long));
        let diagnostic = FormatDiagnostic::collect(1, 1, true, entries);
        assert!(diagnostic.render().len() <= OUTPUT_LIMIT);
        assert!(diagnostic.truncated);
    }
}
