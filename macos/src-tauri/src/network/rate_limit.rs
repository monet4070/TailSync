//! Platform view of the shared peer event rate limiter.
//!
//! The per-peer event/byte budget lives in `tailsync_core::peer::rate_limit`;
//! this module only re-exports it so existing `network::rate_limit` call
//! sites stay unchanged. The file is byte-identical on both platforms
//! (enforced by the cross-platform drift check).

#[allow(unused_imports)] // Contract surface: both platforms share this exact file.
pub use tailsync_core::peer::rate_limit::check_peer_event_budget;
