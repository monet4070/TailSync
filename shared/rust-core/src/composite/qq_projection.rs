//! A helper-produced QQ document, never a generic multi-item flattening.
use crate::protocol::native_archive::{NativeArchive, CONVERTER_QQ_TO_PORTABLE, QQ_MESSAGE_FORMAT};
use crate::protocol::native_clipboard::{FormatState, NativeFormat, NativeSnapshot, SnapshotState};

pub(super) fn project(archive: &NativeArchive) -> Option<NativeSnapshot> {
    if archive.platform != 1
        || !archive.enumeration_complete
        || archive.original.state != SnapshotState::Stable
        || !archive.original.formats.iter().any(|format| {
            format.item == 0
                && format.name == QQ_MESSAGE_FORMAT
                && format.state == FormatState::Captured
        })
        || archive.original.formats.iter().any(|format| {
            format.item > 0
                && !(format.name == "public.file-url" && format.state == FormatState::Absent)
                && !crate::protocol::native_policy::control(&format.name)
        })
    {
        return None;
    }
    let converted: Vec<_> = archive
        .derived
        .iter()
        .filter(|format| {
            format.item == 0
                && format.source_name == QQ_MESSAGE_FORMAT
                && format.converter == CONVERTER_QQ_TO_PORTABLE
        })
        .collect();
    if converted.len() != 2
        || !converted.iter().any(|format| format.name == "public.html")
        || !converted
            .iter()
            .any(|format| format.name == "public.utf8-plain-text")
    {
        return None;
    }
    Some(NativeSnapshot {
        before: archive.original.before,
        after: archive.original.after,
        items: 1,
        state: SnapshotState::Stable,
        formats: converted
            .into_iter()
            .map(|format| NativeFormat {
                item: 0,
                name: format.name.clone(),
                state: FormatState::Captured,
                bytes: format.bytes.clone(),
            })
            .collect(),
    })
}
