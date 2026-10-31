use serde::Serialize;
use tailsync_runtime::contracts::*;

#[derive(Serialize, schemars::JsonSchema)]
struct LocalContractExports {
    pairing: tailsync_core::pairing::PairingStatus,
    capabilities: LocalCapabilities,
    stable_error: StableErrorEnvelope,
    history: tailsync_core::db::HistoryQueryPage,
    favorite: tailsync_core::db::FavoriteMutation,
    history_restore: tailsync_runtime::history::HistoryRestoreReport,
    resource_archive: tailsync_runtime::history::ArchiveEntryImagesReport,
    source_context_grant: tailsync_runtime::source_context::SourceContextGrantReport,
    source_context_revoke: tailsync_runtime::source_context::SourceContextRevokeReport,
    source_context_import: tailsync_runtime::source_context::SourceContextImportReport,
    mac_runtime: MacRuntimeSnapshot,
    windows_runtime: WindowsRuntimeSnapshot,
    peers: PeersResponse,
    preview_error: tailsync_core::db::PreviewErrorInfo,
    preview: tailsync_runtime::preview::PreviewFrameMetadata,
    document: tailsync_runtime::document_preview::DocumentFrameMetadata,
    document_model: tailsync_core::composite::display_document::DisplayDocument,
    document_request: tailsync_runtime::document_preview::DocumentRequest,
}

#[derive(Serialize)]
struct DocumentPolicies {
    version: u16,
    max_bytes: usize,
    max_metadata_bytes: usize,
    max_model_bytes: usize,
    text_limit: usize,
    block_limit: usize,
    run_limit: usize,
    cell_limit: usize,
    asset_limit: usize,
    pixel_limit: u64,
    detail_limit: usize,
    link_limit: usize,
}

fn main() {
    if std::env::args().any(|arg| arg == "--stable-error-policies") {
        let codes = [
            StableErrorCode::InvalidArgument,
            StableErrorCode::NotFound,
            StableErrorCode::TemporarilyBusy,
            StableErrorCode::StorageUnavailable,
            StableErrorCode::Unauthorized,
            StableErrorCode::ProtocolIncompatible,
            StableErrorCode::PublishPartial,
            StableErrorCode::PublishUnknown,
            StableErrorCode::PublishSuperseded,
            StableErrorCode::NativeOnly,
            StableErrorCode::OriginalsIncomplete,
            StableErrorCode::ResourcesIncomplete,
            StableErrorCode::NoSingleImage,
            StableErrorCode::FormatUnavailable,
            StableErrorCode::UnsupportedFormat,
            StableErrorCode::PairingIdentityMismatch,
            StableErrorCode::PairingWindowClosed,
            StableErrorCode::PairingTimedOut,
            StableErrorCode::PairingConnectionClosed,
            StableErrorCode::PairingBusy,
            StableErrorCode::PairingCancelled,
            StableErrorCode::InternalError,
        ];
        let policies = codes.map(StableErrorEnvelope::new);
        println!("{}", serde_json::to_string(&policies).unwrap());
        return;
    }
    if std::env::args().any(|arg| arg == "--document-policies") {
        use tailsync_core::composite::display_document as document;
        let policies = DocumentPolicies {
            version: document::DOCUMENT_VERSION,
            max_bytes: document::DOCUMENT_MAX_BYTES,
            max_metadata_bytes: document::DOCUMENT_MAX_METADATA_BYTES,
            max_model_bytes: document::DOCUMENT_MAX_MODEL_BYTES,
            text_limit: document::DOCUMENT_TEXT_LIMIT,
            block_limit: document::DOCUMENT_BLOCK_LIMIT,
            run_limit: document::DOCUMENT_RUN_LIMIT,
            cell_limit: document::DOCUMENT_CELL_LIMIT,
            asset_limit: document::DOCUMENT_ASSET_LIMIT,
            pixel_limit: document::DOCUMENT_PIXEL_LIMIT,
            detail_limit: document::DOCUMENT_DETAIL_LIMIT,
            link_limit: document::DOCUMENT_LINK_LIMIT,
        };
        println!("{}", serde_json::to_string(&policies).unwrap());
        return;
    }
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
        history_restore: tailsync_runtime::history::HistoryRestoreReport::default(),
        resource_archive: tailsync_runtime::history::ArchiveEntryImagesReport {
            archived: 1,
            deduplicated: 0,
            complete: false,
            failures: vec![tailsync_runtime::history::ArchiveResourceFailure {
                image_index: 1,
                code: "archive_decode".into(),
            }],
        },
        source_context_grant: tailsync_runtime::source_context::SourceContextGrantReport {
            grant_id: "ab".repeat(32),
            origin: "https://example.com".into(),
            expires_at: 1_760_000_000,
            node_count: 1,
            budget_bytes: 16 * 1024 * 1024,
            ticket: "{\"kind\":\"tailsync-source-ticket\"}".into(),
        },
        source_context_revoke: tailsync_runtime::source_context::SourceContextRevokeReport {
            revoked: true,
        },
        source_context_import: tailsync_runtime::source_context::SourceContextImportReport {
            imported: 1,
            deduplicated: 0,
            complete: true,
            failures: vec![],
        },
        pairing: tailsync_core::pairing::PairingStatus {
            pairing_enabled: false,
            phase: tailsync_core::pairing::PairingPhase::Disabled,
            expires_at: None,
            remaining_seconds: 0,
            failed_attempts: 0,
            max_failures: 5,
            peer: None,
            error: None,
            failure_kind: None,
            pending_store_unavailable: false,
            pending: vec![tailsync_core::pairing::PendingPairingSummary {
                hostname: "fixture-peer".into(),
                address: "192.168.1.2".into(),
                interface: "lan".into(),
                fingerprint: "fixture-only".into(),
                locally_trusted: false,
            }],
        },
        capabilities: LocalCapabilities::current("macos", 5, true, true),
        stable_error: StableErrorEnvelope::new(StableErrorCode::TemporarilyBusy),
        history: HistoryQueryPage {
            entries: vec![HistoryEntry {
                id: 7,
                timestamp: "2026-09-08T00:00:00Z".into(),
                composite: Some(CompositeInfo {
                    identity: Some(tailsync_core::composite::identity::HistoryIdentity {
                        version: 1,
                        capture_id: "07".repeat(16),
                        content_id: format!("hcl1:1:{}", "08".repeat(32)),
                        dedupe_key: None,
                        archive_revision: 1,
                        archive_digest: "09".repeat(32),
                    }),
                    native_only: false,
                    native_item_count: 1,
                    formats: vec!["text".into(), "html".into(), "png".into()],
                    // The fixture shows the distinction R5-04 turns on: `png` is present
                    // but withheld, because this item carries two images and no standalone
                    // PNG, so the server would refuse an explicit PNG restore.
                    restorable_formats: vec!["text".into(), "html".into()],
                    withheld_formats: vec![WithheldRestoreFormat {
                        format: "png".into(),
                        code: "no_single_image".into(),
                    }],
                    representation_count: 3,
                    image_count: 2,
                    completeness: "complete".into(),
                }),
                entry_type: "composite".into(),
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
                display: Some(HistoryDisplayInfo {
                    version: 1,
                    kind: "document".into(),
                    thumbnail_eligible: true,
                    cache_key: format!("th1:{}", "00".repeat(32)),
                }),
            }],
            total: Some(1),
            has_more: false,
            display_index_pending: Some(false),
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
                arrival: None,
                code: None,
            }],
            notification_earliest_available_id: 1,
            notification_dropped_total: 0,
            service_instance: 42,
        },
        windows_runtime: WindowsRuntimeSnapshot {
            revision: 1,
            history_version: 1,
            progress: Some(progress),
            sync_warning: Some(tailsync_core::sync_warning::SyncWarning {
                id: 1,
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
        document_model: {
            use tailsync_core::composite::display_document::*;
            DisplayDocument {
                version: 1,
                blocks: vec![
                    DocumentBlock {
                        kind: DocumentBlockKind::Paragraph,
                        paragraph: Some(DocumentParagraph {
                            list: Some(DocumentListKind::Bullet),
                            heading: Some(2),
                            quote_depth: 1,
                            code_block: false,
                            list_depth: 1,
                            runs: vec![DocumentRun {
                                text: "hello".into(),
                                link: Some("https://example.com/reading?q=1&mode=preview".into()),
                                style: DocumentRunStyle {
                                    bold: true,
                                    monospace: true,
                                    highlight: Some(DocumentColor {
                                        red: 255,
                                        green: 255,
                                        blue: 0,
                                    }),
                                    color: Some(DocumentColor {
                                        red: 255,
                                        green: 0,
                                        blue: 0,
                                    }),
                                    ..Default::default()
                                },
                                unsupported: false,
                            }],
                        }),
                        table: None,
                        image: None,
                    },
                    DocumentBlock {
                        kind: DocumentBlockKind::Image,
                        paragraph: None,
                        table: None,
                        image: Some(DocumentImage {
                            asset: Some(0),
                            availability: DocumentImageAvailability::Available,
                            ordinal: 0,
                        }),
                    },
                    DocumentBlock {
                        kind: DocumentBlockKind::Table,
                        paragraph: None,
                        table: Some(DocumentTable {
                            rows: vec![DocumentTableRow {
                                cells: vec![DocumentTableCell {
                                    header: true,
                                    runs: vec![DocumentRun {
                                        text: "h".into(),
                                        link: None,
                                        style: DocumentRunStyle::default(),
                                        unsupported: false,
                                    }],
                                }],
                            }],
                            truncated: false,
                        }),
                        image: None,
                    },
                ],
                assets: vec![DocumentAsset {
                    index: 0,
                    offset: 0,
                    length: 3,
                    width: 1,
                    height: 1,
                    sha256: "0b".repeat(32),
                }],
                partial: vec![DocumentPartial {
                    code: DocumentPartialCode::NativeStylesUnavailable,
                    detail: "synthetic".into(),
                }],
                styles_partial: true,
                first_image: Some(0),
            }
        },
        document: tailsync_runtime::document_preview::DocumentFrameMetadata {
            request_id: Some("fixture-request".into()),
            entry_id: 7,
            kind: "document".into(),
            name: "document".into(),
            stamp: format!("dc1:{}", "0a".repeat(32)),
            owner: "history".into(),
            generation: 1,
            model_bytes: 100,
            size_bytes: 103,
            document: Default::default(),
            batch: None,
        },
        document_request: tailsync_runtime::document_preview::DocumentRequest {
            entry_id: 7,
            owner: "history".into(),
            generation: 1,
            request_id: "fixture-request".into(),
        },
    }
}
