//! Wire protocol for TailSync.
//!
//! This module re-exports [`tailsync_protocol`] so that existing consumers can
//! continue to use `tailsync_core::protocol::*` unchanged. The implementation
//! (framing, commands, payload codecs, and version constants) now lives in the
//! standalone `tailsync-protocol` crate.
pub use tailsync_protocol::*;
