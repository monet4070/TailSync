//! Shared application operations that sit between transports and the Core.
//!
//! This crate deliberately has no Tauri, AppKit, Win32, or UI dependency. A
//! platform transport decodes its request, invokes one of these operations,
//! and maps the typed result back to its own wire format.

pub mod archive_sync;
pub mod automatic_image_archive;
pub mod content_analysis;
pub mod contracts;
pub mod document_preview;
pub mod execution;
pub mod history;
pub mod history_display;
pub mod notification_events;
pub mod notification_text;
pub mod peer_refresh;
pub mod preview;
pub mod source_context;

#[cfg(test)]
mod test_fixtures {
    // These scenarios share process-wide preview admission and pipeline budgets.
    // Keep each scenario's worker/task concurrency; isolate unrelated fixtures.
    pub(crate) static PIPELINE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}
