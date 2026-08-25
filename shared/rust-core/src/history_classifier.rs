//! Clipboard-history text classification for TailSync.
//!
//! This module re-exports [`tailsync_history_classifier`] so that existing
//! consumers can continue to use `tailsync_core::history_classifier::*`
//! unchanged. The implementation (category detection, confidence scoring, and
//! the classifier version) now lives in the standalone
//! `tailsync-history-classifier` crate.
pub use tailsync_history_classifier::*;
