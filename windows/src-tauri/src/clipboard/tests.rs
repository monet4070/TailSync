use super::{
    files_to_broadcast, peer_is_transfer_eligible, run_outgoing_recovery_loop,
    summarize_file_batch_failures, ClipboardEventGate, IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

fn transfer_peer(
    enabled: bool,
    trusted: bool,
    online: bool,
) -> crate::network::tailscale::PeerInfo {
    crate::network::tailscale::PeerInfo {
        hostname: "peer".to_string(),
        tailscale_ip: "100.64.0.2".to_string(),
        online,
        enabled,
        address: "100.64.0.2:53317".to_string(),
        connection_mode: "auto".to_string(),
        trusted,
        fingerprint: String::new(),
        candidates: Vec::new(),
        current_interface: None,
        current_address: None,
        status: Default::default(),
    }
}

#[test]
fn consecutive_native_events_for_identical_content_are_debounced() {
    let mut gate = ClipboardEventGate::default();
    let first = Instant::now();

    assert!(gate.should_process(true, true, first));
    assert!(!gate.should_process(false, true, first + Duration::from_millis(100)));
    assert!(gate.should_process(
        false,
        true,
        first + Duration::from_millis(IDENTICAL_CLIPBOARD_EVENT_DEBOUNCE_MS)
    ));
    assert!(!gate.should_process(false, false, first + Duration::from_secs(2)));
}

#[test]
fn changed_content_bypasses_the_native_event_debounce() {
    let mut gate = ClipboardEventGate::default();
    let first = Instant::now();

    assert!(gate.should_process(true, true, first));
    assert!(gate.should_process(true, true, first + Duration::from_millis(10)));
}

#[test]
fn immediate_transfers_require_enabled_trusted_peers_with_a_route() {
    assert!(peer_is_transfer_eligible(&transfer_peer(true, true, true)));
    assert!(!peer_is_transfer_eligible(&transfer_peer(
        false, true, true
    )));
    assert!(!peer_is_transfer_eligible(&transfer_peer(
        true, false, true
    )));
    assert!(peer_is_transfer_eligible(&transfer_peer(true, true, false)));
}

#[test]
fn iroh_node_ids_are_valid_broadcast_targets_without_ip_parsing() {
    let mut peer = transfer_peer(true, true, false);
    peer.address = "7f5a1b2c3d4e5f60718293a4b5c6d7e8".into();
    peer.candidates = vec![crate::network::PeerCandidate::new(
        crate::network::ConnectionInterface::Iroh,
        peer.address.clone(),
    )];
    assert!(peer_is_transfer_eligible(&peer));
}

#[test]
fn file_batch_failures_are_summarized_once() {
    assert_eq!(
        summarize_file_batch_failures(&[("Mac".into(), "connection lost".into())]),
        "File transfer to Mac failed: connection lost"
    );
    assert_eq!(
        summarize_file_batch_failures(&[
            ("Mac".into(), "connection lost".into()),
            ("Laptop".into(), "timed out".into()),
        ]),
        "File transfer failed on 2 devices: Mac, Laptop"
    );
}

#[tokio::test]
async fn outgoing_recovery_retries_pending_work_after_the_peer_returns() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let recovery_attempts = attempts.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let worker = tokio::spawn(run_outgoing_recovery_loop(
        shutdown_rx,
        Duration::from_millis(1),
        Duration::from_secs(60),
        move || {
            let attempts = recovery_attempts.clone();
            async move {
                // The first pass models the receiver being offline. Keeping
                // the journal pending must schedule a second pass, where the
                // peer has returned and the transfer succeeds.
                attempts.fetch_add(1, Ordering::SeqCst) == 0
            }
        },
    ));

    tokio::time::timeout(Duration::from_millis(250), async {
        while attempts.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("recovery worker did not retry pending work");
    shutdown_tx.send(true).unwrap();
    worker.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn repeated_native_events_for_a_managed_file_never_broadcast() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = managed_directory.join("transfer/report.pdf");
    let paths = vec![path.clone()];

    assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
    assert!(files_to_broadcast(&paths, &managed_directory).is_empty());
}

#[test]
fn user_owned_file_is_still_broadcast() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = std::path::PathBuf::from("documents/report.pdf");

    assert_eq!(
        files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
        vec![path]
    );
}

#[test]
fn managed_directory_name_prefix_is_not_treated_as_managed() {
    let managed_directory = std::path::PathBuf::from("tailsync-data/clipboard-files");
    let path = std::path::PathBuf::from("tailsync-data/clipboard-files-export/report.pdf");

    assert_eq!(
        files_to_broadcast(std::slice::from_ref(&path), &managed_directory),
        vec![path]
    );
}

#[test]
fn canonical_alias_of_a_managed_file_is_not_broadcast() {
    let root = std::env::temp_dir().join(format!(
        "tailsync-managed-path-test-{:016x}",
        rand::random::<u64>()
    ));
    let actual_managed_directory = root.join("clipboard-files");
    let managed_directory = root.join("alias/../clipboard-files");
    std::fs::create_dir_all(root.join("alias")).unwrap();
    let transfer_directory = actual_managed_directory.join("transfer");
    std::fs::create_dir_all(&transfer_directory).unwrap();
    let file = transfer_directory.join("report.pdf");
    std::fs::write(&file, b"report").unwrap();

    assert!(files_to_broadcast(&[file], &managed_directory).is_empty());

    std::fs::remove_dir_all(root).unwrap();
}
