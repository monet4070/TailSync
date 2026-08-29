use super::{
    acquire_peer_file_batch, bind_tcp_listener, cached_discover_peers, clear_peer_cache,
    connection_task, prewarm_connections, queue_peer_frame, race_connect_and_handshake,
    record_protocol_compatibility_error, secure, store_peer_cache, ConnectionInterface,
    ConnectionLimiter, ConnectionPool, PeerCandidate, PeerStatus, PoolSender, QueuedFrame,
    ResolvedCandidate, ResolvedTarget, POOL_CHANNEL_SIZE,
};
use crate::crypto::{self, Settings};
use crate::identity::DeviceIdentity;
use crate::network::tailscale::{LocalInfo, PeerInfo};
use crate::protocol::{unix_timestamp_ms, Command, EventEnvelope, Frame, MessageId};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::net::IpAddr;
use std::sync::Arc;
use tailsync_core::peer::delivery::AckExpectation;
use tailsync_core::peer::delivery::{deliver_pending_frame, PendingFrame};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::{timeout, Duration};

#[test]
fn protocol_compatibility_diagnostic_is_recorded_and_cleared() {
    let hostname = format!("compatibility-test-{}", rand::random::<u64>());
    let message = "Incompatible TailSync protocol: peer uses v2";
    record_protocol_compatibility_error(&hostname, message);
    assert_eq!(
        super::protocol_compatibility_error(&hostname).as_deref(),
        Some(message)
    );
    super::clear_protocol_compatibility_error(&hostname);
    assert_eq!(super::protocol_compatibility_error(&hostname), None);
}

#[tokio::test]
async fn listener_can_rebind_after_a_connection_closes() {
    // Port 0 comes from the same range used by parallel outbound tests,
    // which can claim the port between close and rebind.
    let base_port = 20_000 + (std::process::id() % 9_000) as u16;
    let listener = (base_port..base_port + 100)
        .find_map(|port| bind_tcp_listener(([127, 0, 0, 1], port).into()).ok())
        .expect("a free non-ephemeral test port");
    let address = listener.local_addr().unwrap();
    let (client, accepted) =
        tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
    drop(client.unwrap());
    let (mut accepted, _) = accepted.unwrap();
    let mut eof = [0_u8; 1];
    assert_eq!(
        tokio::io::AsyncReadExt::read(&mut accepted, &mut eof)
            .await
            .unwrap(),
        0
    );
    drop(accepted);
    drop(listener);

    let rebound = bind_tcp_listener(address).unwrap();
    assert_eq!(rebound.local_addr().unwrap(), address);
}

#[test]
fn connection_limiter_caps_each_source_ip() {
    let limiter = ConnectionLimiter::new(64, 8);
    let first_ip: IpAddr = "192.168.1.10".parse().unwrap();
    let second_ip: IpAddr = "192.168.1.11".parse().unwrap();
    let mut permits = (0..8)
        .map(|_| limiter.try_acquire(first_ip).expect("permit"))
        .collect::<Vec<_>>();
    assert!(limiter.try_acquire(first_ip).is_none());
    assert!(limiter.try_acquire(second_ip).is_some());
    permits.pop();
    assert!(limiter.try_acquire(first_ip).is_some());
}

#[tokio::test]
async fn connection_pool_reuses_sender_for_peer() {
    let identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = Arc::new(Mutex::new(crypto::Settings::default()));
    let mut pool = ConnectionPool::new(identity, settings);
    let addr = "127.0.0.1:19890".parse().unwrap();

    let first = pool.sender_for(addr, "macbook".into()).unwrap();
    let second = pool.sender_for(addr, "macbook".into()).unwrap();

    assert_eq!(pool.senders.len(), 1);
    assert!(first.same_channel(&second));
}

#[tokio::test]
async fn file_batches_are_serial_per_peer_and_parallel_between_peers() {
    let identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = Arc::new(Mutex::new(crypto::Settings::default()));
    let pool = Arc::new(Mutex::new(ConnectionPool::new(identity, settings)));

    let first = acquire_peer_file_batch(&pool, "peer-a").await;
    assert!(timeout(
        Duration::from_millis(20),
        acquire_peer_file_batch(&pool, "peer-a")
    )
    .await
    .is_err());
    assert!(timeout(
        Duration::from_millis(20),
        acquire_peer_file_batch(&pool, "peer-b")
    )
    .await
    .is_ok());
    drop(first);
    assert!(timeout(
        Duration::from_millis(20),
        acquire_peer_file_batch(&pool, "peer-a")
    )
    .await
    .is_ok());
}

#[tokio::test]
async fn prewarm_recreates_a_trusted_connection_after_pool_disconnect() {
    let identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = Arc::new(Mutex::new(crypto::Settings::default()));
    let pool = Arc::new(Mutex::new(ConnectionPool::new(identity, settings)));
    let mut trusted = discovered_peer(
        "prewarm-mode-switch-test",
        "192.168.252.40",
        ConnectionInterface::Lan,
    );
    trusted.trusted = true;

    prewarm_connections(pool.clone(), vec![trusted.clone()]).await;
    assert_eq!(pool.lock().await.senders.len(), 1);

    pool.lock().await.disconnect_all();
    assert!(pool.lock().await.senders.is_empty());

    prewarm_connections(pool.clone(), vec![trusted]).await;
    assert_eq!(pool.lock().await.senders.len(), 1);

    let untrusted = discovered_peer(
        "untrusted-prewarm-test",
        "192.168.252.41",
        ConnectionInterface::Lan,
    );
    pool.lock().await.disconnect_all();
    prewarm_connections(pool.clone(), vec![untrusted]).await;
    assert!(pool.lock().await.senders.is_empty());
}

#[tokio::test]
async fn cached_peer_lookup_does_not_run_discovery() {
    clear_peer_cache().await;
    let local = LocalInfo {
        hostname: "local".into(),
        tailscale_ip: "127.0.0.1".into(),
        candidates: Vec::new(),
    };
    let peers = vec![PeerInfo {
        hostname: "cached-peer".into(),
        tailscale_ip: "127.0.0.2".into(),
        online: true,
        enabled: true,
        address: String::new(),
        connection_mode: "cache-test".into(),
        trusted: true,
        fingerprint: String::new(),
        candidates: Vec::new(),
        current_interface: None,
        current_address: None,
        status: Default::default(),
    }];
    store_peer_cache("cache-test", local, peers).await;

    // "cache-test" is not a discoverable mode, so this only succeeds on a cache hit.
    let (_, cached_peers) = cached_discover_peers("cache-test").await.unwrap();

    assert_eq!(cached_peers.len(), 1);
    assert_eq!(cached_peers[0].hostname, "cached-peer");
    clear_peer_cache().await;
}

fn discovered_peer(hostname: &str, address: &str, interface: ConnectionInterface) -> PeerInfo {
    PeerInfo {
        hostname: hostname.into(),
        tailscale_ip: address.into(),
        online: true,
        enabled: true,
        address: address.into(),
        connection_mode: interface.as_str().into(),
        trusted: false,
        fingerprint: String::new(),
        candidates: vec![PeerCandidate::new(interface, address)],
        current_interface: None,
        current_address: None,
        status: Default::default(),
    }
}

async fn serve_noise_once(listener: TcpListener, identity: Arc<DeviceIdentity>) {
    let (stream, _) = listener.accept().await.unwrap();
    let accepted = secure::accept(
        stream,
        &identity,
        secure::PeerIdentity {
            hostname: "server".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
    )
    .await
    .unwrap();
    let mut connection = accepted.connection;
    secure::write_ready(&mut connection).await.unwrap();
}

fn resolved_candidate(
    interface: ConnectionInterface,
    socket_addr: std::net::SocketAddr,
) -> ResolvedCandidate {
    ResolvedCandidate {
        candidate: PeerCandidate::new(interface, socket_addr.ip().to_string()),
        target: ResolvedTarget::Tcp(socket_addr),
    }
}

fn race_settings(server_identity: &DeviceIdentity) -> Arc<Mutex<Settings>> {
    let mut settings = Settings::default();
    settings.trusted_peer_keys.insert(
        "server".into(),
        STANDARD.encode(server_identity.public_key()),
    );
    Arc::new(Mutex::new(settings))
}

#[tokio::test]
async fn connection_race_uses_reachable_lan_before_tailscale_delay() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = race_settings(&server_identity);
    let lan_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lan_address = lan_listener.local_addr().unwrap();
    let tailscale_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tailscale_address = tailscale_listener.local_addr().unwrap();
    let server = tokio::spawn(serve_noise_once(lan_listener, server_identity));

    let (_, winner) = race_connect_and_handshake(
        &[
            resolved_candidate(ConnectionInterface::Lan, lan_address),
            resolved_candidate(ConnectionInterface::Tailscale, tailscale_address),
        ],
        "server",
        &client_identity,
        &settings,
    )
    .await
    .unwrap();

    assert_eq!(winner.candidate.interface, ConnectionInterface::Lan);
    assert!(
        timeout(Duration::from_millis(350), tailscale_listener.accept())
            .await
            .is_err()
    );
    server.await.unwrap();
}

#[tokio::test]
async fn connection_race_falls_back_to_tailscale_after_delay() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = race_settings(&server_identity);
    let tailscale_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tailscale_address = tailscale_listener.local_addr().unwrap();
    // Keep the LAN socket bound without accepting so another parallel test
    // cannot reuse the address while the fallback race is running.
    let unavailable = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_address = unavailable.local_addr().unwrap();
    let server = tokio::spawn(serve_noise_once(tailscale_listener, server_identity));
    let started = tokio::time::Instant::now();

    let (_, winner) = race_connect_and_handshake(
        &[
            resolved_candidate(ConnectionInterface::Lan, unavailable_address),
            resolved_candidate(ConnectionInterface::Tailscale, tailscale_address),
        ],
        "server",
        &client_identity,
        &settings,
    )
    .await
    .unwrap();

    assert_eq!(winner.candidate.interface, ConnectionInterface::Tailscale);
    assert!(started.elapsed() >= Duration::from_millis(200));
    server.await.unwrap();
}

#[tokio::test]
async fn connection_worker_stops_when_the_pool_disconnects_it() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = race_settings(&server_identity);
    let unavailable = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = unavailable.local_addr().unwrap();
    let (priority, priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (bulk, bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let sender = PoolSender {
        priority,
        bulk,
        shutdown,
    };
    let worker = tokio::spawn(connection_task(
        vec![resolved_candidate(ConnectionInterface::Lan, address)],
        "server".into(),
        priority_rx,
        bulk_rx,
        client_identity.clone(),
        settings.clone(),
        shutdown_rx,
    ));
    let mut pool = ConnectionPool::new(client_identity, settings);
    pool.senders
        .insert((ResolvedTarget::Tcp(address), "server".into()), sender);

    tokio::task::yield_now().await;
    pool.disconnect_all();

    timeout(Duration::from_millis(500), worker)
        .await
        .expect("connection worker ignored the pool shutdown request")
        .unwrap();
}

#[tokio::test]
async fn fifteen_minute_old_event_is_not_revived_after_reconnect() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = race_settings(&server_identity);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (first_stream, _) = listener.accept().await.unwrap();
        let first = secure::accept(
            first_stream,
            &server_identity,
            secure::PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        let mut first_connection = first.connection;
        secure::write_ready(&mut first_connection).await.unwrap();
        let before_sleep = first_connection.read_frame().await.unwrap();
        let first_envelope = EventEnvelope::decode(&before_sleep.payload).unwrap();
        drop(first_connection);

        let (second_stream, _) = listener.accept().await.unwrap();
        let second = secure::accept(
            second_stream,
            &server_identity,
            secure::PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        let mut second_connection = second.connection;
        secure::write_ready(&mut second_connection).await.unwrap();
        let retried = second_connection.read_frame().await.unwrap();
        let retried_envelope = EventEnvelope::decode(&retried.payload).unwrap();
        assert_eq!(retried_envelope.message_id, first_envelope.message_id);
        assert_eq!(retried_envelope.timestamp_ms, first_envelope.timestamp_ms);
        assert!(retried_envelope
            .validate_timestamp(unix_timestamp_ms())
            .is_err());
        second_connection
            .write_frame(
                &Frame::try_new(
                    Command::PeerError,
                    0,
                    retried.sequence,
                    b"event timestamp outside window".to_vec(),
                )
                .expect("valid peer error fixture"),
            )
            .await
            .unwrap();

        let after_wake = second_connection.read_frame().await.unwrap();
        let after_wake_envelope = EventEnvelope::decode(&after_wake.payload).unwrap();
        second_connection
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    after_wake.sequence,
                    after_wake_envelope.message_id.ack_payload(),
                )
                .expect("valid event acknowledgement fixture"),
            )
            .await
            .unwrap();
        (
            first_envelope.content,
            retried_envelope.content,
            after_wake_envelope.content,
        )
    });

    let (priority, priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (_bulk, bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(connection_task(
        vec![resolved_candidate(ConnectionInterface::Lan, address)],
        "server".into(),
        priority_rx,
        bulk_rx,
        client_identity,
        settings,
        shutdown_rx,
    ));

    let mut stale = EventEnvelope::new(b"before-sleep".to_vec());
    stale.timestamp_ms = unix_timestamp_ms() - 15 * 60 * 1000;
    let before_sleep = QueuedFrame::new_with_envelope(Command::TextPayload, stale).unwrap();
    priority.send(before_sleep).await.unwrap();
    priority
        .send(QueuedFrame::new(Command::TextPayload, b"after-wake".to_vec()).unwrap())
        .await
        .unwrap();

    let (first, retried, delivered) = timeout(Duration::from_secs(10), server)
        .await
        .expect("new event remained blocked behind the rejected pending event")
        .unwrap();
    assert_eq!(first, b"before-sleep");
    assert_eq!(retried, b"before-sleep");
    assert_eq!(delivered, b"after-wake");
    let _ = shutdown.send(true);
    timeout(Duration::from_secs(1), worker)
        .await
        .expect("connection worker did not stop after the regression test")
        .unwrap();
}

#[tokio::test]
async fn full_peer_queue_does_not_hold_connection_pool_lock() {
    let identity = Arc::new(DeviceIdentity::generate_for_test());
    let mut settings_value = crypto::Settings::default();
    settings_value.trusted_peer_keys.insert(
        "blocked-peer".into(),
        STANDARD.encode(DeviceIdentity::generate_for_test().public_key()),
    );
    let settings = Arc::new(Mutex::new(settings_value));
    let addr = "127.0.0.1:19890".parse().unwrap();
    let (priority, _priority_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (bulk, _bulk_rx) = mpsc::channel(POOL_CHANNEL_SIZE);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    for _ in 0..POOL_CHANNEL_SIZE {
        priority
            .try_send(QueuedFrame::new(Command::TextPayload, vec![1]).unwrap())
            .unwrap();
    }
    let tx = PoolSender {
        priority,
        bulk,
        shutdown,
    };

    let mut pool_value = ConnectionPool::new(identity, settings);
    pool_value
        .senders
        .insert((ResolvedTarget::Tcp(addr), "blocked-peer".into()), tx);
    let pool = Arc::new(Mutex::new(pool_value));
    let queued_pool = pool.clone();
    let peer = PeerInfo {
        hostname: "blocked-peer".into(),
        tailscale_ip: addr.ip().to_string(),
        online: true,
        enabled: true,
        address: addr.ip().to_string(),
        connection_mode: "lan".into(),
        trusted: true,
        fingerprint: String::new(),
        candidates: vec![PeerCandidate::new(
            ConnectionInterface::Lan,
            addr.ip().to_string(),
        )],
        current_interface: None,
        current_address: None,
        status: PeerStatus::Online,
    };
    let blocked_send = tokio::spawn(async move {
        queue_peer_frame(&queued_pool, &peer, Command::TextPayload, vec![2]).await
    });

    tokio::task::yield_now().await;
    let lock = timeout(Duration::from_millis(100), pool.lock())
        .await
        .expect("full peer queue held the global connection pool lock");
    drop(lock);
    blocked_send.abort();
}

#[tokio::test]
async fn connection_pool_rebuilds_a_dead_cached_sender() {
    // Regression: if a worker task has exited it dropped its receivers, so
    // the cached sender's channels read as closed. Reusing it would
    // silently black-hole every frame; sender_for must rebuild instead.
    let identity = Arc::new(DeviceIdentity::generate_for_test());
    let settings = Arc::new(Mutex::new(crypto::Settings::default()));
    let mut pool = ConnectionPool::new(identity, settings);
    let addr: std::net::SocketAddr = "127.0.0.1:19890".parse().unwrap();

    // A sender whose worker has "exited": drop both receivers so the
    // channels report closed, then seed it into the cache under the key
    // sender_for computes for this (target, hostname).
    let (priority, priority_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
    let (bulk, bulk_rx) = mpsc::channel::<QueuedFrame>(POOL_CHANNEL_SIZE);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    drop(priority_rx);
    drop(bulk_rx);
    let dead = PoolSender {
        priority,
        bulk,
        shutdown,
    };
    assert!(dead.priority.is_closed() && dead.bulk.is_closed());
    pool.senders.insert(
        (ResolvedTarget::Tcp(addr), "windows-pc".into()),
        dead.clone(),
    );

    let rebuilt = pool.sender_for(addr, "windows-pc".into()).unwrap();

    assert_eq!(pool.senders.len(), 1, "the dead entry must be replaced");
    assert!(
        !dead.same_channel(&rebuilt),
        "sender_for must hand back a freshly built worker, not the dead one"
    );
    assert!(
        !rebuilt.priority.is_closed(),
        "the rebuilt worker's channel must be live"
    );
}

#[tokio::test]
async fn file_chunks_use_a_separate_queue_from_priority_messages() {
    let (priority, mut priority_rx) = mpsc::channel(1);
    let (bulk, mut bulk_rx) = mpsc::channel(1);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let sender = PoolSender {
        priority,
        bulk,
        shutdown,
    };

    sender
        .channel_for(Command::FileChunk)
        .send(QueuedFrame::new(Command::FileChunk, vec![1]).unwrap())
        .await
        .unwrap();
    sender
        .channel_for(Command::TextPayload)
        .send(QueuedFrame::new(Command::TextPayload, vec![2]).unwrap())
        .await
        .unwrap();

    let priority = priority_rx.recv().await.unwrap();
    let bulk = bulk_rx.recv().await.unwrap();
    assert_eq!(priority.command(), Command::TextPayload);
    assert!(matches!(
        priority.acknowledgement(),
        AckExpectation::Event(_)
    ));
    assert_eq!(bulk.command(), Command::FileChunk);
    assert!(matches!(bulk.acknowledgement(), AckExpectation::None));
}

#[tokio::test]
async fn reliable_delivery_retries_the_same_event_until_acknowledged() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let expected_key = server_identity.public_key().to_vec();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let accepted = secure::accept(
            stream,
            &server_identity,
            secure::PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        let mut connection = accepted.connection;
        secure::write_ready(&mut connection).await.unwrap();
        let first = connection.read_frame().await.unwrap();
        let retry = connection.read_frame().await.unwrap();
        assert_eq!(retry.sequence, first.sequence);
        assert_eq!(retry.payload, first.payload);
        let message_id = EventEnvelope::decode(&retry.payload).unwrap().message_id;
        connection
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    retry.sequence,
                    message_id.ack_payload(),
                )
                .expect("valid event acknowledgement fixture"),
            )
            .await
            .unwrap();
    });
    let mut client = secure::connect(
        tokio::net::TcpStream::connect(address).await.unwrap(),
        &client_identity,
        secure::PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        "server",
        &expected_key,
    )
    .await
    .unwrap();
    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
        42,
    );

    deliver_pending_frame(
        &mut client,
        &pending,
        &tailsync_core::peer::delivery::DeliveryConfig::DEFAULT,
    )
    .await
    .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn reliable_delivery_rejects_an_ack_for_another_message() {
    let server_identity = Arc::new(DeviceIdentity::generate_for_test());
    let client_identity = DeviceIdentity::generate_for_test();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let expected_key = server_identity.public_key().to_vec();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let accepted = secure::accept(
            stream,
            &server_identity,
            secure::PeerIdentity {
                hostname: "server".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
        )
        .await
        .unwrap();
        let mut connection = accepted.connection;
        secure::write_ready(&mut connection).await.unwrap();
        let event = connection.read_frame().await.unwrap();
        connection
            .write_frame(
                &Frame::try_new(
                    Command::EventAck,
                    0,
                    event.sequence,
                    MessageId::random().ack_payload(),
                )
                .expect("valid event acknowledgement fixture"),
            )
            .await
            .unwrap();
    });
    let mut client = secure::connect(
        tokio::net::TcpStream::connect(address).await.unwrap(),
        &client_identity,
        secure::PeerIdentity {
            hostname: "client".into(),
            tailscale_ip: String::new(),
            iroh_endpoint_id: None,
        },
        "server",
        &expected_key,
    )
    .await
    .unwrap();
    let pending = PendingFrame::new(
        QueuedFrame::new(Command::TextPayload, b"reliable".to_vec()).unwrap(),
        7,
    );

    let error = deliver_pending_frame(
        &mut client,
        &pending,
        &tailsync_core::peer::delivery::DeliveryConfig::DEFAULT,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("different event"));
    server.await.unwrap();
}
