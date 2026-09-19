use serde::Serialize;
use tailsync_runtime::contracts::*;

#[derive(Serialize, schemars::JsonSchema)]
struct LocalContractExports {
    capabilities: LocalCapabilities,
    history: tailsync_core::db::HistoryQueryPage,
    favorite: tailsync_core::db::FavoriteMutation,
    mac_runtime: MacRuntimeSnapshot,
    windows_runtime: WindowsRuntimeSnapshot,
    peers: PeersResponse,
    preview_error: tailsync_core::db::PreviewErrorInfo,
    preview: tailsync_runtime::preview::PreviewFrameMetadata,
}

fn main() {
    if std::env::args().any(|arg| arg == "--fixtures") {
        println!("{}", serde_json::to_string_pretty(&fixtures()).unwrap());
        return;
    }
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<LocalContractExports>();
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

fn fixtures() -> LocalContractExports {
    use tailsync_core::{db::*, peer::types::*};
    let peer = PeerInfo {
        hostname: "fixture-peer".into(),
        tailscale_ip: "192.0.2.1".into(),
        online: true,
        enabled: true,
        address: "192.0.2.1".into(),
        connection_mode: "lan".into(),
        trusted: true,
        fingerprint: "fixture-only".into(),
        candidates: vec![PeerCandidate::new(ConnectionInterface::Lan, "192.0.2.1")],
        current_interface: None,
        current_address: None,
        status: PeerStatus::Online,
    };
    let progress = FileProgress {
        batch_id: "fixture-batch".into(),
        name: "synthetic.txt".into(),
        sent: 1,
        total: 2,
        active: true,
        direction: "sending".into(),
        device: "fixture-peer".into(),
        completed_files: 0,
        total_files: 1,
        speed_bytes_per_second: 1,
        status: "transferring".into(),
        can_stop: true,
    };
    LocalContractExports {
        capabilities: LocalCapabilities::current("macos", 4, true, true),
        history: HistoryQueryPage {
            entries: vec![HistoryEntry {
                id: 7,
                timestamp: "2026-09-08T00:00:00Z".into(),
                entry_type: "text".into(),
                description: "synthetic fixture".into(),
                data_hash: "00".repeat(32),
                size_bytes: 5,
                source_peer: "fixture-peer".into(),
                category: "text".into(),
                categories: vec!["text".into()],
                category_confidence: 100,
                classifier_version: 1,
                pinned: true,
                batch_id: None,
                batch_index: None,
                batch_total: None,
                batch_count: None,
                batch_status: "complete".into(),
            }],
            total: Some(1),
            has_more: false,
        },
        favorite: FavoriteMutation {
            affected_ids: vec![7],
            favorite: true,
        },
        mac_runtime: MacRuntimeSnapshot {
            revision: 1,
            history_version: 1,
            progress: Some(progress.clone()),
            storage: StorageStatus {
                root: "/isolated-fixture".into(),
                used_bytes: 5,
                quota_bytes: 1073741824,
                available: true,
                error: None,
            },
            sync_enabled: true,
            status: DaemonStatus {
                tcp_server_healthy: true,
                clipboard_monitor_healthy: true,
                clipboard_monitor_failures: 0,
                active_routes: [(
                    "fixture-peer".into(),
                    ActiveRoute {
                        interface: ConnectionInterface::Lan,
                        address: "192.0.2.1".into(),
                        latency: 1,
                    },
                )]
                .into(),
            },
            notifications: vec![RuntimeNotification {
                id: 1,
                level: "error".into(),
                message: "synthetic error".into(),
            }],
        },
        windows_runtime: WindowsRuntimeSnapshot {
            revision: 1,
            history_version: 1,
            progress: Some(progress),
            sync_warning: Some(tailsync_core::sync_warning::SyncWarning {
                kind: "expired_event",
                peer: "fixture-peer".into(),
                occurred_at_ms: 0,
            }),
            notifications: vec![],
        },
        peers: PeersResponse {
            local: LocalDeviceSnapshot {
                hostname: "fixture-local".into(),
                tailscale_ip: "192.0.2.2".into(),
                routes: vec![],
                connection_mode: "auto".into(),
                public_key: "fixture-only".into(),
                fingerprint: "fixture-only".into(),
                iroh_endpoint_id: None,
            },
            peers: vec![PeerSnapshot::new(peer, None, None, 4)],
            paired_peer_endpoints: [("fixture-peer".into(), "192.0.2.1".into())].into(),
            discovery_error: None,
        },
        preview_error: PreviewErrorInfo::payload_unavailable(7, "synthetic cancellation"),
        preview: tailsync_runtime::preview::PreviewFrameMetadata {
            request_id: Some("fixture-request".into()),
            entry_id: 7,
            kind: "text".into(),
            name: "text.txt".into(),
            size_bytes: 5,
            width: None,
            height: None,
            batch: Some(PreviewBatchNavigation {
                batch_id: "fixture-batch".into(),
                item_index: 0,
                item_count: 1,
                first_entry_id: 7,
                last_entry_id: 7,
                previous_entry_id: None,
                next_entry_id: None,
            }),
        },
    }
}
