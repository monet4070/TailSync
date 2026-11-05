//! macOS byte admission. Presence of a suppression marker blocks the entire
//! capture before payload reads. Deferred dependencies are never materialized
//! by this byte policy. Unknown names are bounded opaque Data, never decoded.
include!("native_policy.generated.rs");

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && !name.bytes().any(|b| b < 32 || b == 127)
}
pub fn suppressed(name: &str) -> bool {
    SUPPRESSED.contains(&name)
}
pub fn control(name: &str) -> bool {
    CONTROLS.contains(&name)
}
/// Windows clipboard control formats. These are observations about the whole
/// copy, not user payload; only their presence or fixed DWORD value is read.
pub const WINDOWS_EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
pub const WINDOWS_CAN_INCLUDE_HISTORY: &str = "CanIncludeInClipboardHistory";
pub const WINDOWS_CAN_UPLOAD_CLOUD: &str = "CanUploadToCloudClipboard";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsPrivacy {
    Allow,
    Suppress,
    /// A control format was present but could not be interpreted. The current
    /// revision must not be read through another path.
    Invalid,
}

/// Conservative whole-event policy (plan §2.3). A present exclusion marker
/// suppresses without reading any other control value. DWORD controls must be
/// exactly four bytes and exactly 0 or 1; anything else is not permission.
pub fn windows_privacy_decision(
    exclude_present: bool,
    history: Option<&[u8]>,
    cloud: Option<&[u8]>,
) -> WindowsPrivacy {
    if exclude_present {
        return WindowsPrivacy::Suppress;
    }
    for value in [history, cloud].into_iter().flatten() {
        if value.len() != 4 {
            return WindowsPrivacy::Invalid;
        }
        match u32::from_le_bytes([value[0], value[1], value[2], value[3]]) {
            0 => return WindowsPrivacy::Suppress,
            1 => {}
            _ => return WindowsPrivacy::Invalid,
        }
    }
    WindowsPrivacy::Allow
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsReadPlan {
    ReadBody,
    Suppress,
    Failed,
}

/// The only place that turns a privacy decision into a read plan. Production
/// uses this before enumerating payload formats, so a suppression marker cannot
/// fall through to a body read.
pub fn windows_read_plan(decision: WindowsPrivacy) -> WindowsReadPlan {
    match decision {
        WindowsPrivacy::Allow => WindowsReadPlan::ReadBody,
        WindowsPrivacy::Suppress => WindowsReadPlan::Suppress,
        WindowsPrivacy::Invalid => WindowsReadPlan::Failed,
    }
}
pub fn macos_publishable(name: &str, bytes: usize) -> bool {
    if !valid_name(name) || SUPPRESSED.contains(&name) || DEFERRED.contains(&name) || control(name)
    {
        return false;
    }
    bytes
        <= COMMON
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(OPAQUE_LIMIT, |(_, limit)| *limit)
}
/// The current Windows helper accepts these byte formats; decoding and framing
/// are additional preparation gates, not established by a name match.
pub fn windows_publishable(name: &str) -> bool {
    matches!(
        name,
        "CF_UNICODETEXT" | "HTML Format" | "Rich Text Format" | "PNG" | "CF_DIB" | "CF_DIBV5"
    )
}
/// CNS1 history is a legacy projection with a fixed allowlist. Exact opaque
/// macOS originals use CNA1; expanding publication does not expand old storage.
pub fn legacy_storable(name: &str) -> bool {
    windows_publishable(name)
        || matches!(
            name,
            "public.utf8-plain-text"
                | "public.html"
                | "public.rtf"
                | "com.apple.flat-rtfd"
                | "public.png"
                | "public.tiff"
        )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn opaque(name: &str) -> bool {
        valid_name(name)
            && !SUPPRESSED.contains(&name)
            && !DEFERRED.contains(&name)
            && !control(name)
            && !COMMON.iter().any(|(n, _)| *n == name)
    }
    #[test]
    fn control_predicate_is_exactly_the_generated_marker_set() {
        assert_eq!(
            CONTROLS,
            &[
                "org.tailsync.clipboard-source",
                "org.tailsync.rtfd-converted"
            ]
        );
        assert!(CONTROLS.iter().all(|name| control(name)));
        for name in [
            "public.utf8-plain-text",
            "public.png",
            "com.apple.flat-rtfd",
            "com.example.vendor-format",
        ] {
            assert!(!control(name), "{name}");
        }
    }
    #[test]
    fn wcap_privacy_controls_are_conservative() {
        assert_eq!(
            windows_privacy_decision(true, None, None),
            WindowsPrivacy::Suppress
        );
        assert_eq!(
            windows_privacy_decision(false, Some(&0u32.to_le_bytes()), None),
            WindowsPrivacy::Suppress
        );
        assert_eq!(
            windows_privacy_decision(false, None, Some(&0u32.to_le_bytes())),
            WindowsPrivacy::Suppress
        );
        assert_eq!(
            windows_privacy_decision(false, Some(&1u32.to_le_bytes()), Some(&1u32.to_le_bytes())),
            WindowsPrivacy::Allow
        );
        for invalid in [&[][..], &[1, 0, 0][..], &[2, 0, 0, 0][..], &[0; 8][..]] {
            assert_eq!(
                windows_privacy_decision(false, Some(invalid), None),
                WindowsPrivacy::Invalid
            );
        }
    }
    #[test]
    fn wcap_suppression_marker_never_reaches_a_body_read() {
        let mut body_reads = 0;
        if windows_read_plan(windows_privacy_decision(true, None, None))
            == WindowsReadPlan::ReadBody
        {
            body_reads += 1;
        }
        assert_eq!(body_reads, 0);
        assert_eq!(
            windows_read_plan(WindowsPrivacy::Invalid),
            WindowsReadPlan::Failed
        );
        assert_eq!(
            windows_read_plan(WindowsPrivacy::Allow),
            WindowsReadPlan::ReadBody
        );
    }
    #[test]
    fn image_retention_budget_is_independent_of_opaque_and_cover_limits() {
        for name in ["public.jpeg", "com.compuserve.gif", "org.webmproject.webp"] {
            assert!(macos_publishable(name, OPAQUE_LIMIT + 1));
            assert!(macos_publishable(name, 32 * 1024 * 1024));
            assert!(!macos_publishable(name, 32 * 1024 * 1024 + 1));
            assert!(!opaque(name));
            assert!(!windows_publishable(name));
        }
    }
    #[test]
    fn exact_policy_keeps_unknown_bytes_but_excludes_sensitive_and_dependencies() {
        for name in [
            "com.example.vendor-format",
            "application/x-fixture",
            "org.nspasteboard.ConcealedType.extra",
        ] {
            assert!(opaque(name));
            assert!(macos_publishable(name, 0));
            assert!(macos_publishable(name, OPAQUE_LIMIT));
            assert!(!macos_publishable(name, OPAQUE_LIMIT + 1));
            assert!(!legacy_storable(name));
            assert!(!windows_publishable(name));
        }
        for name in SUPPRESSED {
            assert!(suppressed(name));
            assert!(!macos_publishable(name, 0));
        }
        for name in DEFERRED {
            assert!(!macos_publishable(name, 0));
            assert!(!opaque(name));
        }
        for name in ["", "bad\nname", "bad\0name"] {
            assert!(!macos_publishable(name, 0));
        }
    }
}
