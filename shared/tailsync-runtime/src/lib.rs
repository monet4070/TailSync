//! Shared application operations that sit between transports and the Core.
//!
//! This crate deliberately has no Tauri, AppKit, Win32, or UI dependency. A
//! platform transport decodes its request, invokes one of these operations,
//! and maps the typed result back to its own wire format.

pub mod contracts;
pub mod execution;
pub mod history;
pub mod peer_refresh;
pub mod preview;
