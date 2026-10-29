//! Bounded Qt image clipboard shape observed in WeChat for macOS.
use crate::protocol::native_clipboard::{FormatState, NativeSnapshot, SnapshotState};

/// Only the observed self-contained, single static image shape is admitted.
/// Qt metadata is a format discriminator, not proof of the originating app.
/// We never follow file URLs or accept additional message/file payloads here.
pub(super) fn matches(snapshot: &NativeSnapshot) -> bool {
    use crate::protocol::{image, native_policy};
    if snapshot.validate().is_err()
        || snapshot.state != SnapshotState::Stable
        || snapshot.items != 1
    {
        return false;
    }
    let mut image_present = false;
    let mut qt_marker = false;
    let mut deferred_url = false;
    for f in &snapshot.formats {
        if native_policy::control(&f.name) {
            continue;
        }
        match f.name.as_str() {
            "public.file-url" if f.state == FormatState::Absent => deferred_url = true,
            "com.trolltech.anymime.application--x-qt-image"
            | "com.trolltech.anymime.text--uri-list"
                if f.state == FormatState::Captured && f.bytes.is_empty() =>
            {
                qt_marker |= f.name == "com.trolltech.anymime.application--x-qt-image";
            }
            "public.tiff" | "public.png" if f.state == FormatState::Captured => {
                let expected = if f.name == "public.tiff" {
                    image::ImageKind::Tiff
                } else {
                    image::ImageKind::Png
                };
                if !native_policy::macos_publishable(&f.name, f.bytes.len())
                    || !image::probe(&f.bytes).is_some_and(|m| {
                        m.kind == expected && m.frames == 1 && !m.animation && m.within_budget()
                    })
                {
                    return false;
                }
                image_present = true;
            }
            _ => return false,
        }
    }
    image_present && qt_marker && deferred_url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{
        self, fingerprint,
        origin::{Origin, OriginTracker},
        Mime,
    };
    use crate::protocol::native_archive::{DerivedFormat, NativeArchive, CONVERTER_TIFF_TO_PNG};
    use crate::protocol::native_clipboard::NativeFormat;
    const TIFF: &[u8] =
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/color.tiff");
    const PNG: &[u8] =
        include_bytes!("../../../tailsync-protocol/tests/fixtures/images/transparent.png");
    fn archive() -> NativeArchive {
        NativeArchive {
            platform: 1,
            enumeration_complete: true,
            original: NativeSnapshot {
                before: 12,
                after: 12,
                items: 1,
                state: SnapshotState::Stable,
                formats: vec![
                    NativeFormat {
                        item: 0,
                        name: "public.file-url".into(),
                        state: FormatState::Absent,
                        bytes: vec![],
                    },
                    NativeFormat {
                        item: 0,
                        name: "com.trolltech.anymime.text--uri-list".into(),
                        state: FormatState::Captured,
                        bytes: vec![],
                    },
                    NativeFormat {
                        item: 0,
                        name: "public.tiff".into(),
                        state: FormatState::Captured,
                        bytes: TIFF.to_vec(),
                    },
                    NativeFormat {
                        item: 0,
                        name: "com.trolltech.anymime.application--x-qt-image".into(),
                        state: FormatState::Captured,
                        bytes: vec![],
                    },
                ],
            },
            derived: vec![DerivedFormat {
                item: 0,
                name: "public.png".into(),
                source_name: "public.tiff".into(),
                converter: CONVERTER_TIFF_TO_PNG.into(),
                bytes: PNG.to_vec(),
            }],
        }
    }
    #[test]
    fn wechat_image_with_deferred_url_converts_and_has_external_origin() {
        let a = archive();
        let before = a.encode().unwrap();
        assert!(matches(&a.original));
        let bundle = composite::from_archive(&a)
            .expect("WeChat's image must be portable despite its unused file URL");
        assert_eq!(bundle.representation(Mime::Png), Some(PNG));
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original).unwrap();
        assert_eq!(
            OriginTracker::default().classify(Some(12), digest, None),
            Origin::External
        );
        assert!(fingerprint::origin_fingerprint_from_native(&a.original).is_err());
        assert_eq!(a.encode().unwrap(), before);
        let published = composite::to_macos_snapshot(&bundle).unwrap();
        let written = fingerprint::origin_fingerprint_from_native(&published).unwrap();
        let mut tracker = OriginTracker::default();
        tracker.record(20, written, [9; 16]);
        assert_eq!(
            tracker.classify(
                Some(20),
                fingerprint::origin_fingerprint_from_capture(&published).unwrap(),
                Some([9; 16])
            ),
            Origin::SelfWrite
        );
    }
    #[test]
    fn wechat_observation_retains_order_independence_and_write_guards() {
        let a = archive();
        let digest = fingerprint::origin_fingerprint_from_capture(&a.original).unwrap();
        let mut changed = a.original.clone();
        changed.formats.reverse();
        changed.before = 14;
        changed.after = 14;
        assert_eq!(
            fingerprint::origin_fingerprint_from_capture(&changed).unwrap(),
            digest
        );
        changed
            .formats
            .iter_mut()
            .find(|f| f.name == "public.tiff")
            .unwrap()
            .bytes
            .push(0);
        assert_ne!(
            fingerprint::origin_fingerprint_from_capture(&changed).unwrap(),
            digest
        );
        let mut tracker = OriginTracker::default();
        tracker.record(12, [0; 32], [1; 16]);
        assert_eq!(
            tracker.classify(Some(12), digest, None),
            Origin::UnknownMismatch
        );
        let mut tracker = OriginTracker::default();
        tracker.record_intent([1; 16], digest);
        assert_eq!(
            tracker.classify(Some(12), digest, None),
            Origin::UnknownMismatch
        );
    }
    #[test]
    fn wechat_shape_rejects_missing_pixels_and_ambiguous_or_failed_formats() {
        let a = archive();
        for index in 0..a.original.formats.len() {
            let mut invalid = a.original.clone();
            invalid.formats[index].state = FormatState::Failed;
            invalid.formats[index].bytes.clear();
            assert!(!matches(&invalid));
            assert!(fingerprint::origin_fingerprint_from_capture(&invalid).is_err());
        }
        for name in [
            "public.utf8-plain-text",
            "com.example.private",
            "public.file-url",
        ] {
            let mut invalid = a.original.clone();
            invalid.formats.push(NativeFormat {
                item: 0,
                name: name.into(),
                state: FormatState::Captured,
                bytes: b"unrelated".to_vec(),
            });
            assert!(!matches(&invalid));
        }
        let mut invalid = a.original.clone();
        invalid.formats[1].bytes = b"file:///private/unrelated.png".to_vec();
        assert!(!matches(&invalid));
        let mut invalid = a.original.clone();
        invalid.formats[2].bytes = b"broken TIFF".to_vec();
        assert!(!matches(&invalid));
        let mut invalid = a.original.clone();
        invalid.formats[2].bytes =
            include_bytes!("../../../tailsync-protocol/tests/fixtures/images/pages.tiff").to_vec();
        assert!(!matches(&invalid));
        let mut invalid = a.original.clone();
        invalid.formats.pop();
        assert!(!matches(&invalid));
        let mut invalid = a.original.clone();
        invalid.items = 2;
        assert!(!matches(&invalid));
        let mut invalid = a.original.clone();
        invalid.after += 1;
        assert!(!matches(&invalid));
        let mut invalid = a.clone();
        invalid.enumeration_complete = false;
        assert!(composite::from_archive(&invalid).is_err());
        let mut invalid = a.clone();
        invalid.derived.clear();
        assert!(composite::from_archive(&invalid).is_err());
    }
}
