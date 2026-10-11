//! Platform view of the shared peer types.
//!
//! All peer discovery, health, and delivery types live in
//! `tailsync_core::peer::types`; this module only re-exports them so existing
//! `network::types` call sites stay unchanged. The file is byte-identical on
//! both platforms (enforced by the cross-platform drift check), even though
//! macOS does not consume the Windows-only `ActiveRoute`/`PeerHealthSnapshot`
//! types — the re-export keeps the shared contract surface complete.

#[allow(unused_imports)] // Contract surface: both platforms share this exact file.
pub use tailsync_core::peer::types::{
    ActiveRoute, ConnectionInterface, PeerCandidate, PeerHealthSnapshot, PeerStatus,
};
