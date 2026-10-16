//! Supported portable format scope and native-format mapping rules.

use super::PortableMime;

/// How well a portable format is supported across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportLevel {
    /// Captured and restored losslessly on both platforms today.
    Portable,
    /// Captured locally; cross-platform playback still needs native validation.
    LocalOnly,
}

/// Support level for a portable representation format.
///
/// RTF stays `LocalOnly` until target-application experiments validate the
/// cross-platform playback configuration (plan §三, M6). Unknown registered
/// native formats are never mapped and must not be forwarded.
pub const fn support_level(mime: PortableMime) -> SupportLevel {
    match mime {
        PortableMime::PlainTextUtf8 | PortableMime::HtmlUtf8 | PortableMime::Png => {
            SupportLevel::Portable
        }
        PortableMime::Rtf => SupportLevel::LocalOnly,
    }
}

/// Map a native clipboard format name to a portable representation.
///
/// Returns `None` for anything not explicitly verified, so "pass the format by
/// name" can never silently become an arbitrary format passthrough.
pub fn portable_mime_for_native(name: &str) -> Option<PortableMime> {
    match name {
        "text/plain" | "public.utf8-plain-text" | "CF_UNICODETEXT" | "UTF8_STRING" => {
            Some(PortableMime::PlainTextUtf8)
        }
        "text/html" | "public.html" | "HTML Format" | "CF_HTML" => Some(PortableMime::HtmlUtf8),
        "text/rtf" | "public.rtf" | "Rich Text Format" | "CF_RTF" => Some(PortableMime::Rtf),
        "image/png" | "public.png" | "PNG" => Some(PortableMime::Png),
        _ => None,
    }
}
