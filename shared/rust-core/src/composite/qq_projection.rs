//! A helper-produced QQ document, never a generic multi-item flattening.
use crate::protocol::native_archive::{NativeArchive, CONVERTER_QQ_TO_PORTABLE, QQ_MESSAGE_FORMAT};
use crate::protocol::native_clipboard::{FormatState, NativeFormat, NativeSnapshot, SnapshotState};

pub(super) fn deferred_image_descriptor(format: &NativeFormat) -> bool {
    // QQ advertises the descriptor on the message item as well as extra items.
    // This predicate is used only after identifying the captured QQ message.
    format.name == "public.file-url" && format.state == FormatState::Absent
}

pub(super) fn message_shape(snapshot: &NativeSnapshot) -> bool {
    snapshot.state == SnapshotState::Stable
        && snapshot.formats.iter().any(|format| {
            format.item == 0
                && format.name == QQ_MESSAGE_FORMAT
                && format.state == FormatState::Captured
        })
        && !snapshot.formats.iter().any(|format| {
            format.item > 0
                && !deferred_image_descriptor(format)
                && !crate::protocol::native_policy::control(&format.name)
        })
}

pub(super) fn project(archive: &NativeArchive) -> Option<NativeSnapshot> {
    if archive.platform != 1 || !archive.enumeration_complete || !message_shape(&archive.original) {
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
