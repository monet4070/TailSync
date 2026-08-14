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

#[cfg(test)]
mod contract_tests {
    use super::*;
    use tailsync_core::peer::types as core_types;

    /// Compile-time identity proof: the platform re-exports must be the very
    /// same types as the shared core ones, not lookalikes. If this file
    /// drifts from the core contract, these assignments stop compiling.
    fn same<T>(value: T) -> T {
        value
    }

    #[test]
    fn re_exported_types_are_the_shared_core_types() {
        let _: core_types::ConnectionInterface = same(ConnectionInterface::Lan);
        let _: core_types::PeerStatus = same(PeerStatus::Offline);
        let _: core_types::PeerCandidate =
            same(PeerCandidate::new(ConnectionInterface::Lan, "192.168.1.2"));
        let _: core_types::PeerHealthSnapshot = same(PeerHealthSnapshot {
            status: PeerStatus::Online,
            online: true,
            connected: false,
            latency_ms: None,
        });
        let _: core_types::ActiveRoute = same(ActiveRoute {
            interface: ConnectionInterface::Lan,
            address: "192.168.1.2".into(),
            latency: 1,
        });
    }
}
