use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::db;
use crate::identity::{
    persist_protected_bytes_create_only, read_protected_bytes, CreateOutcome, IdentityError,
};

pub const ALPN: &[u8] = b"tailsync/4";
pub const RTT_ALPN: &[u8] = b"tailsync/4/rtt";
const SECRET_KEY_SIZE: usize = 32;
static IDENTITY_RECOVERY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

type ShutdownFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

enum ShutdownState {
    Open,
    Waiting(ShutdownFuture),
    Complete,
    Failed(String),
}

pub fn canonical_endpoint_id(value: &str) -> Result<String, String> {
    EndpointId::from_str(value.trim())
        .map(|endpoint_id| endpoint_id.to_string())
        .map_err(|error| format!("Invalid Iroh endpoint ID: {error}"))
}

pub fn persistent_endpoint_id() -> Result<String, String> {
    load_or_recover_secret_key(&identity_path())
        .map(|secret_key| secret_key.public().to_string())
        .map_err(|error| format!("Could not load Iroh identity: {error}"))
}

#[derive(Clone, Debug)]
pub struct IrohEndpoint {
    endpoint: Endpoint,
}

pub struct AcceptedConnection {
    connection: Connection,
    pub remote_endpoint_id: String,
}

pub struct IrohBiStream {
    send: SendStream,
    recv: RecvStream,
    connection: Connection,
    shutdown_state: ShutdownState,
}

pub struct IrohRttProbe {
    connection: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectedPathMetrics {
    rtt: Duration,
    direct: bool,
}

/// The transport used for a measured route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RttPath {
    /// A peer-to-peer UDP path (possibly via NAT hole punching).
    Direct,
    /// A path relayed through an Iroh relay server.
    Relay,
}

/// A latency sample for the selected route, together with the path that was
/// measured. The path matters because a freshly established probe connection
/// may still be on the relay when a direct path would be reachable later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RttSample {
    pub rtt: Duration,
    pub path: RttPath,
}

impl IrohEndpoint {
    pub async fn bind() -> Result<Self, String> {
        let secret_key = load_or_recover_secret_key(&identity_path())
            .map_err(|error| format!("Could not load Iroh identity: {error}"))?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec(), RTT_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|error| format!("Could not start Iroh endpoint: {error}"))?;
        Ok(Self { endpoint })
    }

    pub fn endpoint_id(&self) -> String {
        self.endpoint.id().to_string()
    }

    pub async fn connect(&self, endpoint_id: &str) -> Result<IrohBiStream, String> {
        let endpoint_id = EndpointId::from_str(&canonical_endpoint_id(endpoint_id)?)
            .map_err(|error| format!("Invalid Iroh endpoint ID: {error}"))?;
        self.connect_addr(endpoint_id.into()).await
    }

    async fn connect_addr(&self, endpoint_addr: EndpointAddr) -> Result<IrohBiStream, String> {
        let connection = self
            .endpoint
            .connect(endpoint_addr, ALPN)
            .await
            .map_err(|error| format!("Iroh connection failed: {error}"))?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|error| format!("Could not open Iroh stream: {error}"))?;
        Ok(IrohBiStream::new(send, recv, connection))
    }

    pub async fn connect_rtt(&self, endpoint_id: &str) -> Result<IrohRttProbe, String> {
        let endpoint_id = EndpointId::from_str(&canonical_endpoint_id(endpoint_id)?)
            .map_err(|error| format!("Invalid Iroh endpoint ID: {error}"))?;
        connect_rtt_from(&self.endpoint, endpoint_id.into()).await
    }

    #[cfg(test)]
    async fn connect_rtt_addr(&self, endpoint_addr: EndpointAddr) -> Result<IrohRttProbe, String> {
        connect_rtt_from(&self.endpoint, endpoint_addr).await
    }

    pub async fn accept(&self) -> Result<Option<AcceptedConnection>, String> {
        let Some(incoming) = self.endpoint.accept().await else {
            return Ok(None);
        };
        let connection = incoming
            .await
            .map_err(|error| format!("Could not accept Iroh connection: {error}"))?;
        let remote_endpoint_id = connection.remote_id().to_string();
        Ok(Some(AcceptedConnection {
            connection,
            remote_endpoint_id,
        }))
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

impl AcceptedConnection {
    pub fn is_rtt_probe(&self) -> bool {
        self.connection.alpn() == RTT_ALPN
    }

    pub async fn wait_for_close(self) {
        let _ = self.connection.closed().await;
    }

    pub async fn accept_stream(self) -> Result<IrohBiStream, String> {
        let connection = self.connection;
        let (send, recv) = connection
            .accept_bi()
            .await
            .map_err(|error| format!("Could not accept Iroh stream: {error}"))?;
        Ok(IrohBiStream::new(send, recv, connection))
    }
}

impl IrohBiStream {
    fn new(send: SendStream, recv: RecvStream, connection: Connection) -> Self {
        Self {
            send,
            recv,
            connection,
            shutdown_state: ShutdownState::Open,
        }
    }

    /// Return the selected QUIC path's RTT, allowing a direct path a short
    /// opportunity to replace the initial relay path after connection setup.
    pub async fn measure_rtt(&self, direct_path_wait: Duration) -> Option<RttSample> {
        measure_connection_rtt(&self.connection, direct_path_wait).await
    }
}

impl IrohRttProbe {
    /// Close only the RTT connection. The endpoint is shared with normal
    /// TailSync traffic and must stay alive across repeated measurements.
    pub fn close_now(&self) {
        self.connection.close(0u32.into(), b"probe cancelled");
    }

    pub async fn measure_rtt(self, direct_path_wait: Duration) -> Option<RttSample> {
        let sample = measure_connection_rtt(&self.connection, direct_path_wait).await;
        self.connection.close(0u32.into(), b"probe complete");
        sample
    }
}

impl Drop for IrohRttProbe {
    fn drop(&mut self) {
        self.close_now();
    }
}

async fn connect_rtt_from(
    endpoint: &Endpoint,
    endpoint_addr: EndpointAddr,
) -> Result<IrohRttProbe, String> {
    let connection = endpoint
        .connect(endpoint_addr, RTT_ALPN)
        .await
        .map_err(|error| format!("Iroh connection failed: {error}"))?;
    Ok(IrohRttProbe { connection })
}

fn selected_path_metrics(connection: &Connection) -> Option<SelectedPathMetrics> {
    let paths = connection.paths();
    select_path_metrics(
        paths
            .iter()
            .map(|path| SelectedPathMetrics {
                rtt: path.rtt(),
                direct: path.is_ip(),
            })
            .zip(paths.iter().map(|path| path.is_selected())),
    )
}

async fn measure_connection_rtt(
    connection: &Connection,
    direct_path_wait: Duration,
) -> Option<RttSample> {
    let deadline = tokio::time::Instant::now() + direct_path_wait;
    let mut latest = None;
    loop {
        if let Some(metrics) = selected_path_metrics(connection) {
            latest = Some(sample_from_metrics(metrics));
            if metrics.direct {
                return latest;
            }
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return latest;
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(50))).await;
    }
}

fn sample_from_metrics(metrics: SelectedPathMetrics) -> RttSample {
    RttSample {
        rtt: metrics.rtt,
        path: if metrics.direct {
            RttPath::Direct
        } else {
            RttPath::Relay
        },
    }
}

fn select_path_metrics(
    paths: impl Iterator<Item = (SelectedPathMetrics, bool)>,
) -> Option<SelectedPathMetrics> {
    paths
        .filter_map(|(metrics, selected)| selected.then_some(metrics))
        .next()
}

impl AsyncRead for IrohBiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.recv), cx, buf)
    }
}

impl AsyncWrite for IrohBiStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if matches!(&self.shutdown_state, ShutdownState::Complete) {
            return Poll::Ready(Ok(()));
        }
        if let ShutdownState::Failed(message) = &self.shutdown_state {
            return Poll::Ready(Err(io::Error::other(message.clone())));
        }

        if matches!(&self.shutdown_state, ShutdownState::Open) {
            if let Err(error) = self.send.finish() {
                let message = error.to_string();
                self.shutdown_state = ShutdownState::Failed(message.clone());
                return Poll::Ready(Err(io::Error::other(message)));
            }

            let stopped = self.send.stopped();
            self.shutdown_state = ShutdownState::Waiting(Box::pin(async move {
                match stopped.await {
                    Ok(None) => Ok(()),
                    Ok(Some(code)) => Err(io::Error::other(format!(
                        "peer stopped Iroh send stream with code {code}"
                    ))),
                    Err(error) => Err(io::Error::other(error)),
                }
            }));
        }

        let result = match &mut self.shutdown_state {
            ShutdownState::Waiting(future) => future.as_mut().poll(cx),
            ShutdownState::Complete => return Poll::Ready(Ok(())),
            ShutdownState::Failed(message) => {
                return Poll::Ready(Err(io::Error::other(message.clone())));
            }
            ShutdownState::Open => unreachable!("shutdown state initialized above"),
        };

        match result {
            Poll::Ready(Ok(())) => {
                self.shutdown_state = ShutdownState::Complete;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                let message = error.to_string();
                self.shutdown_state = ShutdownState::Failed(message.clone());
                Poll::Ready(Err(io::Error::other(message)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn load_or_create_secret_key(path: &Path) -> Result<SecretKey, IdentityError> {
    match read_secret_key(path) {
        Ok(key) => return Ok(key),
        Err(IdentityError::NotFound) => {}
        Err(error) => return Err(error),
    }

    let key = SecretKey::generate();
    match persist_protected_bytes_create_only(path, &key.to_bytes())? {
        CreateOutcome::Created => Ok(key),
        CreateOutcome::AlreadyExists => read_secret_key(path),
    }
}

fn load_or_recover_secret_key(path: &Path) -> Result<SecretKey, IdentityError> {
    let _guard = IDENTITY_RECOVERY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match load_or_create_secret_key(path) {
        Ok(key) => Ok(key),
        Err(IdentityError::Corrupt(reason)) => {
            let archived = archive_corrupt_identity(path)?;
            if let Some(archived) = archived {
                log::warn!(
                    "Archived a corrupt Iroh route identity at {} ({reason}); generating a new route identity",
                    archived.display()
                );
            }
            load_or_create_secret_key(path)
        }
        Err(error) => Err(error),
    }
}

fn archive_corrupt_identity(path: &Path) -> Result<Option<std::path::PathBuf>, IdentityError> {
    let parent = path.parent().ok_or_else(|| {
        IdentityError::Corrupt("the Iroh identity path has no parent directory".to_string())
    })?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("iroh-identity-v1.bin");
    for _ in 0..16 {
        let archived = parent.join(format!(
            "{file_name}.corrupt-{}-{:016x}",
            chrono::Utc::now().timestamp_millis(),
            rand::random::<u64>()
        ));
        match std::fs::rename(path, &archived) {
            Ok(()) => return Ok(Some(archived)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(IdentityError::Io {
                    operation: "archiving a corrupt Iroh identity",
                    source: error,
                });
            }
        }
    }
    Err(IdentityError::Io {
        operation: "archiving a corrupt Iroh identity",
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique corrupt identity archive path",
        ),
    })
}

fn read_secret_key(path: &Path) -> Result<SecretKey, IdentityError> {
    let bytes = read_protected_bytes(path)?;
    let bytes: [u8; SECRET_KEY_SIZE] = bytes.try_into().map_err(|bytes: Vec<u8>| {
        IdentityError::Corrupt(format!(
            "Iroh secret key must contain {SECRET_KEY_SIZE} bytes, found {}",
            bytes.len()
        ))
    })?;
    Ok(SecretKey::from_bytes(&bytes))
}

fn identity_path() -> std::path::PathBuf {
    db::get_data_dir().join("iroh-identity-v1.bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Settings;
    use crate::identity::DeviceIdentity;
    use crate::pairing::{PairingManager, PairingPhase, PendingPairing};
    use crate::secure::{self, HandshakePurpose, PeerIdentity};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Each accepted probe is drained before the next connection attempt. The
    // client must reuse its long-lived endpoint so repeated measurements do
    // not restart relay discovery or race a newly bound endpoint's teardown.
    #[tokio::test]
    async fn repeated_rtt_probes_reuse_the_persistent_endpoint() {
        let server = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec(), RTT_ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let server_port = server.endpoint.addr().ip_addrs().next().unwrap().port();
        let server_addr = EndpointAddr::from_parts(
            server.endpoint.id(),
            [iroh::TransportAddr::Ip(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                server_port,
            )))],
        );
        let client = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal).bind().await.unwrap(),
        };
        let client_endpoint_id = client.endpoint_id();
        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let accepted = server.accept().await.unwrap().unwrap();
                assert!(accepted.is_rtt_probe());
                assert_eq!(accepted.remote_endpoint_id, client_endpoint_id);
                accepted.wait_for_close().await;
            }
        });

        let first = tokio::time::timeout(
            Duration::from_secs(10),
            client.connect_rtt_addr(server_addr.clone()),
        )
        .await
        .expect("first RTT connect timed out")
        .unwrap();
        assert!(first
            .measure_rtt(Duration::from_millis(100))
            .await
            .is_some());
        let second = tokio::time::timeout(
            Duration::from_secs(10),
            client.connect_rtt_addr(server_addr),
        )
        .await
        .expect("second RTT connect timed out")
        .unwrap();
        assert!(second
            .measure_rtt(Duration::from_millis(100))
            .await
            .is_some());

        client.close().await;
        tokio::time::timeout(Duration::from_secs(1), server_task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn reverse_direction_rtt_probe_does_not_close_an_existing_business_stream() {
        let mac = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec(), RTT_ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let windows = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec(), RTT_ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let mac_addr = mac.endpoint.addr();
        let windows_addr = windows.endpoint.addr();
        let mac_endpoint_id = mac.endpoint_id();
        let mac_for_server = mac.clone();
        let (business_done_tx, business_done_rx) = tokio::sync::oneshot::channel();
        let server_task = tokio::spawn(async move {
            let accepted = mac_for_server.accept().await.unwrap().unwrap();
            let mut stream = accepted.accept_stream().await.unwrap();
            let mut request = [0u8; 4];
            tokio::io::AsyncReadExt::read_exact(&mut stream, &mut request)
                .await
                .unwrap();
            assert_eq!(&request, b"ping");
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"pong")
                .await
                .unwrap();
            tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
            let _ = business_done_rx.await;
        });
        let windows_for_probe = windows.clone();
        let probe_task = tokio::spawn(async move {
            let accepted = windows_for_probe.accept().await.unwrap().unwrap();
            assert!(accepted.is_rtt_probe());
            assert_eq!(accepted.remote_endpoint_id, mac_endpoint_id);
        });

        let mut business = windows.connect_addr(mac_addr).await.unwrap();
        let probe = mac.connect_rtt_addr(windows_addr).await.unwrap();
        assert!(probe
            .measure_rtt(Duration::from_millis(100))
            .await
            .is_some());

        tokio::io::AsyncWriteExt::write_all(&mut business, b"ping")
            .await
            .unwrap();
        let mut response = [0u8; 4];
        tokio::io::AsyncReadExt::read_exact(&mut business, &mut response)
            .await
            .unwrap();
        assert_eq!(&response, b"pong");
        let _ = business_done_tx.send(());
        server_task.await.unwrap();
        probe_task.await.unwrap();
    }

    #[tokio::test]
    async fn pairing_completes_over_a_real_iroh_stream() {
        let server = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let client = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let server_endpoint_id = server.endpoint_id();
        let client_endpoint_id = client.endpoint_id();
        let server_identity = Arc::new(DeviceIdentity::generate_for_test());
        let client_identity = Arc::new(DeviceIdentity::generate_for_test());
        let server_settings = Arc::new(Mutex::new(Settings::default()));
        let client_settings = Arc::new(Mutex::new(Settings::default()));
        let server_manager = PairingManager::with_policy(
            server_settings.clone(),
            server_identity.clone(),
            Duration::from_secs(5),
            5,
            false,
        );
        let client_manager = PairingManager::with_policy(
            client_settings.clone(),
            client_identity.clone(),
            Duration::from_secs(5),
            5,
            false,
        );
        server_manager.enable().await;
        client_manager.enable().await;

        let server_task = {
            let server = server.clone();
            let server_manager = server_manager.clone();
            let server_identity = server_identity.clone();
            let client_endpoint_id = client_endpoint_id.clone();
            let server_endpoint_id = server_endpoint_id.clone();
            tokio::spawn(async move {
                let accepted = server.accept().await.unwrap().unwrap();
                assert_eq!(accepted.remote_endpoint_id, client_endpoint_id);
                let stream = accepted.accept_stream().await.unwrap();
                let accepted = secure::accept_with_pairing_window(
                    stream,
                    &server_identity,
                    PeerIdentity {
                        hostname: "server".into(),
                        tailscale_ip: String::new(),
                        iroh_endpoint_id: Some(server_endpoint_id),
                    },
                    server_manager.subscribe_window(),
                )
                .await
                .unwrap();
                assert_eq!(accepted.purpose, HandshakePurpose::Pairing);
                let mut connection = accepted.connection;
                secure::write_ready(&mut connection).await.unwrap();
                server_manager
                    .install_session(PendingPairing {
                        connection,
                        hostname: accepted.peer_identity.hostname,
                        remote_public_key: accepted.remote_public_key,
                        handshake_hash: accepted.handshake_hash,
                        address: client_endpoint_id,
                        interface: "iroh".into(),
                    })
                    .await
                    .unwrap();
            })
        };

        let stream = client.connect_addr(server.endpoint.addr()).await.unwrap();
        let accepted = secure::connect_pairing(
            stream,
            &client_identity,
            PeerIdentity {
                hostname: "client".into(),
                tailscale_ip: String::new(),
                iroh_endpoint_id: Some(client_endpoint_id),
            },
        )
        .await
        .unwrap();
        client_manager
            .install_session(PendingPairing {
                connection: accepted.connection,
                hostname: accepted.peer_identity.hostname,
                remote_public_key: accepted.remote_public_key,
                handshake_hash: accepted.handshake_hash,
                address: server_endpoint_id,
                interface: "iroh".into(),
            })
            .await
            .unwrap();
        server_task.await.unwrap();

        let server_code = server_manager
            .status()
            .await
            .peer
            .as_ref()
            .unwrap()
            .verification_code
            .clone();
        let client_code = client_manager
            .status()
            .await
            .peer
            .as_ref()
            .unwrap()
            .verification_code
            .clone();
        assert_eq!(server_code, client_code);

        server_manager.confirm().await.unwrap();
        client_manager.confirm().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if server_manager.status().await.phase == PairingPhase::Paired
                    && client_manager.status().await.phase == PairingPhase::Paired
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Iroh pairing should complete after both persisted acknowledgements");

        assert_eq!(
            server_settings.lock().await.trusted_peer_keys.get("client"),
            Some(&client_identity.public_key_base64())
        );
        assert_eq!(
            client_settings.lock().await.trusted_peer_keys.get("server"),
            Some(&server_identity.public_key_base64())
        );
        client.close().await;
        server.close().await;
    }

    #[tokio::test]
    async fn rejected_alpn_does_not_prevent_the_next_business_connection() {
        let server = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal)
                .alpns(vec![ALPN.to_vec(), RTT_ALPN.to_vec()])
                .bind()
                .await
                .unwrap(),
        };
        let server_addr = server.endpoint.addr();
        let unsupported_client = Endpoint::builder(presets::Minimal).bind().await.unwrap();
        let unsupported_for_connect = unsupported_client.clone();
        let rejected_connect = tokio::spawn(async move {
            unsupported_for_connect
                .connect(server_addr, b"tailsync/unsupported")
                .await
        });
        assert!(server.accept().await.is_err());
        unsupported_client.close().await;
        assert!(rejected_connect.await.unwrap().is_err());

        let client = IrohEndpoint {
            endpoint: Endpoint::builder(presets::Minimal).bind().await.unwrap(),
        };
        let accept_business = async {
            loop {
                match server.accept().await {
                    Ok(Some(accepted)) => break accepted,
                    Ok(None) => panic!("server endpoint closed"),
                    Err(_) => continue,
                }
            }
        };
        let (accepted, connected) =
            tokio::join!(accept_business, client.connect_addr(server.endpoint.addr()));
        assert!(!accepted.is_rtt_probe());
        assert!(connected.is_ok());
    }

    #[test]
    fn rtt_measurement_uses_the_selected_path() {
        let relay = SelectedPathMetrics {
            rtt: Duration::from_millis(240),
            direct: false,
        };
        let direct = SelectedPathMetrics {
            rtt: Duration::from_millis(4),
            direct: true,
        };

        assert_eq!(
            select_path_metrics([(relay, false), (direct, true)].into_iter()),
            Some(direct)
        );
    }

    #[test]
    fn rtt_samples_label_the_measured_path() {
        let relay = SelectedPathMetrics {
            rtt: Duration::from_millis(240),
            direct: false,
        };
        let direct = SelectedPathMetrics {
            rtt: Duration::from_millis(4),
            direct: true,
        };
        assert_eq!(
            sample_from_metrics(relay),
            RttSample {
                rtt: Duration::from_millis(240),
                path: RttPath::Relay,
            }
        );
        assert_eq!(
            sample_from_metrics(direct),
            RttSample {
                rtt: Duration::from_millis(4),
                path: RttPath::Direct,
            }
        );
    }

    #[test]
    fn persisted_identity_keeps_the_same_endpoint_id() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-iroh-identity-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let path = directory.join("identity.bin");

        let first = load_or_create_secret_key(&path).unwrap();
        let second = load_or_create_secret_key(&path).unwrap();

        assert_eq!(first.public(), second.public());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn corrupt_iroh_identity_is_archived_before_regeneration() {
        let directory = std::env::temp_dir().join(format!(
            "tailsync-corrupt-iroh-identity-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("iroh-identity-v1.bin");
        std::fs::write(&path, b"not an encrypted identity").unwrap();

        let recovered = load_or_recover_secret_key(&path).unwrap();
        let reloaded = load_or_recover_secret_key(&path).unwrap();
        let archives = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("iroh-identity-v1.bin.corrupt-")
            })
            .count();

        assert_eq!(recovered.public(), reloaded.public());
        assert_eq!(archives, 1);
        let _ = std::fs::remove_dir_all(directory);
    }
}
