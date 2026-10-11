use serde::de::Error as DeserializeError;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use snow::TransportState;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::identity;
use crate::protocol::{self, Command, Frame, ProtocolError};

const MAX_TRANSPORT_RECORD: usize = u16::MAX as usize;
const MAX_TRANSPORT_PLAINTEXT: usize = MAX_TRANSPORT_RECORD - 16;
const TRANSPORT_WRITE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Fine-grained protocol extensions advertised inside the authenticated
/// Noise handshake. Missing and unknown fields are deliberately treated as
/// unsupported so older and newer peers safely fall back to wire-v4 behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilitySet {
    pub file_sliding_window: bool,
    pub image_compressed_chunks: bool,
    pub pairing_commit_proof: bool,
    pub composite_clipboard: bool,
    pub archive_resource_updates: bool,
}

impl CapabilitySet {
    pub const fn disabled() -> Self {
        Self {
            file_sliding_window: false,
            image_compressed_chunks: false,
            pairing_commit_proof: false,
            composite_clipboard: false,
            archive_resource_updates: false,
        }
    }

    pub const fn is_empty(&self) -> bool {
        !self.file_sliding_window
            && !self.image_compressed_chunks
            && !self.pairing_commit_proof
            && !self.composite_clipboard
            && !self.archive_resource_updates
    }

    /// Negotiate each extension independently. A remote advertisement can
    /// never enable a capability that the local connection did not offer.
    pub const fn negotiate(local: Self, remote: Self) -> Self {
        Self {
            file_sliding_window: local.file_sliding_window && remote.file_sliding_window,
            image_compressed_chunks: local.image_compressed_chunks
                && remote.image_compressed_chunks,
            pairing_commit_proof: local.pairing_commit_proof && remote.pairing_commit_proof,
            composite_clipboard: local.composite_clipboard && remote.composite_clipboard,
            archive_resource_updates: local.archive_resource_updates
                && remote.archive_resource_updates
                && local.composite_clipboard
                && remote.composite_clipboard,
        }
    }
}

/// Runtime kill switches for protocol extensions. Build-gated file and image
/// extensions additionally require compile-time support.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapabilitySwitches {
    pub file_sliding_window: bool,
    pub image_compressed_chunks: bool,
    pub pairing_commit_proof: bool,
    pub composite_clipboard: bool,
}

/// Startup helper-health report, set by the platform layer.
///
/// `None` means the platform has not probed yet. Composite clipboard is
/// `compiled_feature && runtime_enabled && helper_healthy` (ADR-004 K8), and the
/// startup order requires the no-body self-check to run *before* the migration
/// policy is frozen — so an unreported probe is deliberately not serviceable
/// rather than optimistically assumed healthy.
const HELPER_UNKNOWN: u8 = 0;
const HELPER_HEALTHY: u8 = 1;
const HELPER_UNHEALTHY: u8 = 2;

static ORIGIN_HEALTH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static JOURNAL_HEALTH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub fn set_origin_health(healthy: bool) {
    ORIGIN_HEALTH.store(healthy, std::sync::atomic::Ordering::Release);
}
pub fn set_journal_health(healthy: bool) {
    JOURNAL_HEALTH.store(healthy, std::sync::atomic::Ordering::Release);
}

static HELPER_HEALTH: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(HELPER_UNKNOWN);

/// Record the result of the platform's no-body helper self-check.
///
/// Call during startup, before opening the database and before the first
/// handshake, so the decision is consistent for both the capability and the
/// schema policy.
pub fn set_helper_health(healthy: bool) {
    HELPER_HEALTH.store(
        if healthy {
            HELPER_HEALTHY
        } else {
            HELPER_UNHEALTHY
        },
        std::sync::atomic::Ordering::SeqCst,
    );
}

/// The last reported helper health, or `None` if the platform has not probed.
pub fn helper_health() -> Option<bool> {
    match HELPER_HEALTH.load(std::sync::atomic::Ordering::SeqCst) {
        HELPER_HEALTHY => Some(true),
        HELPER_UNHEALTHY => Some(false),
        _ => None,
    }
}

/// Restore unknown health after a test fixture; platform tests use the dev-only
/// test-support edge so their fake native adapter must not leak healthy state.
#[cfg(any(test, feature = "test-support"))]
pub fn reset_helper_health_for_test() {
    HELPER_HEALTH.store(HELPER_UNKNOWN, std::sync::atomic::Ordering::SeqCst);
}

/// Whether composite clipboard is serviceable right now: the full K8 predicate.
///
/// This is what the database migration policy and the platform's capture switch
/// both consult, so "the feature is on" can never mean "the schema upgrades even
/// though the platform cannot service it".
pub fn composite_serviceable() -> bool {
    composite_capture_serviceable()
        && ORIGIN_HEALTH.load(std::sync::atomic::Ordering::Acquire)
        && JOURNAL_HEALTH.load(std::sync::atomic::Ordering::Acquire)
}
/// A healthy reader may preserve history while automatic publication is disabled.
pub fn composite_capture_serviceable() -> bool {
    CapabilitySwitches::runtime_defaults()
        .advertised()
        .composite_clipboard
        && helper_health() == Some(true)
}

impl CapabilitySwitches {
    /// Build flags are the file/image default policy; operators can disable any
    /// extension before starting the process for an immediate rollback.
    pub fn runtime_defaults() -> Self {
        Self {
            file_sliding_window: std::env::var_os("TAILSYNC_DISABLE_FILE_WINDOW").is_none(),
            image_compressed_chunks: std::env::var_os("TAILSYNC_DISABLE_IMAGE_CHUNKS").is_none(),
            pairing_commit_proof: std::env::var_os("TAILSYNC_DISABLE_PAIRING_COMMIT_PROOF")
                .is_none(),
            composite_clipboard: std::env::var_os("TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD").is_none(),
        }
    }

    pub fn serviceable_advertisement(self) -> CapabilitySet {
        let mut capabilities = self.advertised();
        capabilities.composite_clipboard &= helper_health() == Some(true)
            && ORIGIN_HEALTH.load(std::sync::atomic::Ordering::Acquire)
            && JOURNAL_HEALTH.load(std::sync::atomic::Ordering::Acquire);
        capabilities.archive_resource_updates &= capabilities.composite_clipboard;
        capabilities
    }

    pub const fn advertised(self) -> CapabilitySet {
        CapabilitySet {
            file_sliding_window: cfg!(feature = "protocol-file-sliding-window")
                && self.file_sliding_window,
            image_compressed_chunks: cfg!(feature = "protocol-image-compressed-chunks")
                && self.image_compressed_chunks,
            pairing_commit_proof: self.pairing_commit_proof,
            composite_clipboard: cfg!(feature = "protocol-composite-clipboard")
                && self.composite_clipboard,
            archive_resource_updates: cfg!(feature = "protocol-composite-clipboard")
                && self.composite_clipboard,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct HandshakeAdvertisement {
    #[serde(flatten)]
    identity: PeerIdentity,
    #[serde(default, skip_serializing_if = "CapabilitySet::is_empty")]
    capabilities: CapabilitySet,
    #[serde(default = "legacy_wire_version")]
    max_wire_version: u8,
}

const fn legacy_wire_version() -> u8 {
    protocol::LEGACY_VERSION
}

const fn negotiated_wire_version(remote: u8) -> u8 {
    if remote >= protocol::VERSION {
        protocol::VERSION
    } else {
        protocol::LEGACY_VERSION
    }
}

const fn capabilities_for_wire(capabilities: CapabilitySet, wire_version: u8) -> CapabilitySet {
    CapabilitySet {
        file_sliding_window: capabilities.file_sliding_window,
        image_compressed_chunks: wire_version == protocol::VERSION
            && capabilities.image_compressed_chunks,
        pairing_commit_proof: capabilities.pairing_commit_proof,
        composite_clipboard: wire_version == protocol::VERSION && capabilities.composite_clipboard,
        archive_resource_updates: wire_version == protocol::VERSION
            && capabilities.composite_clipboard
            && capabilities.archive_resource_updates,
    }
}

fn encode_handshake_advertisement(
    identity: PeerIdentity,
    capabilities: CapabilitySet,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&HandshakeAdvertisement {
        identity,
        capabilities,
        max_wire_version: protocol::VERSION,
    })
}

fn decode_handshake_advertisement(
    bytes: &[u8],
) -> Result<(PeerIdentity, CapabilitySet, u8), serde_json::Error> {
    let advertisement: HandshakeAdvertisement = serde_json::from_slice(bytes)?;
    Ok((
        advertisement.identity,
        advertisement.capabilities,
        negotiated_wire_version(advertisement.max_wire_version),
    ))
}

#[derive(Debug, Clone)]
pub struct PeerIdentity {
    pub hostname: String,
    pub tailscale_ip: String,
    pub iroh_endpoint_id: Option<String>,
}

impl Serialize for PeerIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PeerIdentity", 5)?;
        state.serialize_field("hostname", &self.hostname)?;
        state.serialize_field("tailscale_ip", &self.tailscale_ip)?;
        if let Some(endpoint_id) = &self.iroh_endpoint_id {
            state.serialize_field("iroh_endpoint_id", endpoint_id)?;
        }
        state.serialize_field("protocol_version", &protocol::LEGACY_VERSION)?;
        state.serialize_field("app_version", env!("CARGO_PKG_VERSION"))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PeerIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireIdentity {
            hostname: String,
            tailscale_ip: String,
            #[serde(default)]
            iroh_endpoint_id: Option<String>,
            #[serde(default)]
            protocol_version: Option<u8>,
            #[serde(default)]
            app_version: Option<String>,
        }

        let identity = WireIdentity::deserialize(deserializer)?;
        if let Some(version) = identity.protocol_version {
            if version != protocol::LEGACY_VERSION {
                let app = identity
                    .app_version
                    .as_deref()
                    .map(|version| format!(" ({version})"))
                    .unwrap_or_default();
                return Err(D::Error::custom(format!(
                    "Incompatible TailSync protocol: peer{app} uses v{version}, this version requires v{}. Update TailSync on both devices.",
                    protocol::LEGACY_VERSION
                )));
            }
        }
        Ok(Self {
            hostname: identity.hostname,
            tailscale_ip: identity.tailscale_ip,
            iroh_endpoint_id: identity.iroh_endpoint_id,
        })
    }
}

pub trait SessionIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> SessionIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedSessionIo = Box<dyn SessionIo>;

pub struct SecureConnection {
    stream: BoxedSessionIo,
    transport: TransportState,
    read_buffer: Vec<u8>,
    partial_header: [u8; 2],
    partial_header_len: usize,
    partial_record: Vec<u8>,
    partial_expected: Option<usize>,
    peer_identity: PeerIdentity,
    remote_public_key: Vec<u8>,
    session_id: String,
    negotiated_capabilities: CapabilitySet,
    wire_version: u8,
}

pub struct AcceptedConnection {
    pub connection: SecureConnection,
    pub peer_identity: PeerIdentity,
    pub remote_public_key: Vec<u8>,
    pub handshake_hash: Vec<u8>,
    pub purpose: HandshakePurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakePurpose {
    Connection,
    Pairing,
}

impl SecureConnection {
    /// Noise-authenticated static identity, stable across reconnects and names.
    pub fn remote_public_key(&self) -> &[u8] {
        &self.remote_public_key
    }
    pub fn peer_identity(&self) -> &PeerIdentity {
        &self.peer_identity
    }

    /// Stable correlation identifier shared by both sides of this Noise
    /// session. The raw handshake transcript is never exposed.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Extensions negotiated for this exact authenticated connection. The
    /// value is created by the handshake and is never cached across sessions.
    pub const fn negotiated_capabilities(&self) -> CapabilitySet {
        self.negotiated_capabilities
    }

    pub const fn wire_version(&self) -> u8 {
        self.wire_version
    }

    pub async fn read_frame(&mut self) -> Result<Frame, ProtocolError> {
        self.read_frame_with_admission(|_, _| Ok(())).await
    }

    pub async fn read_frame_with_admission(
        &mut self,
        admission: impl FnOnce(Command, usize) -> Result<(), ProtocolError>,
    ) -> Result<Frame, ProtocolError> {
        let mut admission = Some(admission);
        let mut expected_size = None;
        loop {
            if expected_size.is_none() {
                if let Some((command, payload_length, total_size)) =
                    self.pending_frame_metadata()?
                {
                    if let Some(admit) = admission.take() {
                        admit(command, payload_length)?;
                    }
                    expected_size = Some(total_size);
                }
            }
            if let Some(total_size) = expected_size {
                if self.read_buffer.len() >= total_size {
                    let frame_bytes: Vec<u8> = self.read_buffer.drain(..total_size).collect();
                    return Frame::decode_with_version(&frame_bytes, self.wire_version)
                        .map(|(frame, _)| frame);
                }
            }
            self.read_transport_record().await?;
        }
    }

    pub async fn write_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if matches!(
            frame.command,
            Command::PairingCommitted | Command::PairingCommittedAck
        ) && !self.negotiated_capabilities.pairing_commit_proof
        {
            return Err("Pairing commit proof was not negotiated".into());
        }
        if matches!(
            frame.command,
            Command::CompositePayload | Command::CompositeChunk | Command::CompositeAck
        ) && !self.negotiated_capabilities.composite_clipboard
        {
            return Err("Composite clipboard was not negotiated".into());
        }
        if matches!(
            frame.command,
            Command::ArchiveUpdatePayload | Command::ArchiveUpdateChunk | Command::ArchiveUpdateAck
        ) && (!self.negotiated_capabilities.archive_resource_updates
            || !self.negotiated_capabilities.composite_clipboard)
        {
            return Err("Archive resource updates were not negotiated".into());
        }
        if self.wire_version == protocol::LEGACY_VERSION
            && matches!(
                frame.command,
                Command::ImageChunk
                    | Command::CompositePayload
                    | Command::CompositeChunk
                    | Command::CompositeAck
                    | Command::ArchiveUpdatePayload
                    | Command::ArchiveUpdateChunk
                    | Command::ArchiveUpdateAck
            )
        {
            return Err(ProtocolError::UnsupportedVersion(self.wire_version).into());
        }
        if frame.payload.len() > frame.command.payload_limit() {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command: frame.command,
                actual: frame.payload.len(),
                limit: frame.command.payload_limit(),
            }
            .into());
        }

        let encoded = frame.encode_with_version(self.wire_version);
        for chunk in encoded.chunks(MAX_TRANSPORT_PLAINTEXT) {
            let mut encrypted = vec![0u8; chunk.len() + 32];
            let length = self.transport.write_message(chunk, &mut encrypted)?;
            encrypted.truncate(length);
            let record_length = u16::try_from(encrypted.len())
                .map_err(|_| "Encrypted transport record is too large")?;
            write_all_with_timeout(&mut self.stream, &record_length.to_be_bytes()).await?;
            write_all_with_timeout(&mut self.stream, &encrypted).await?;
        }
        flush_with_timeout(&mut self.stream).await?;
        Ok(())
    }

    /// Finish the underlying write side after the application protocol has
    /// completed its final acknowledgement exchange. Iroh waits for the
    /// peer's transport acknowledgement here so the final frame is not lost
    /// when the stream is dropped immediately afterwards.
    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        self.stream.shutdown().await
    }

    fn pending_frame_metadata(&self) -> Result<Option<(Command, usize, usize)>, ProtocolError> {
        if self.read_buffer.len() < protocol::HEADER_SIZE {
            return Ok(None);
        }
        if self.read_buffer[..4] != protocol::MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }
        if self.read_buffer[4] != self.wire_version {
            return Err(ProtocolError::UnsupportedVersion(self.read_buffer[4]));
        }
        let command_code = u16::from_be_bytes([self.read_buffer[6], self.read_buffer[7]]);
        let command =
            Command::from_u16(command_code).ok_or(ProtocolError::UnknownCommand(command_code))?;
        if self.wire_version == protocol::LEGACY_VERSION
            && matches!(
                command,
                Command::ImageChunk
                    | Command::CompositePayload
                    | Command::CompositeChunk
                    | Command::CompositeAck
                    | Command::ArchiveUpdatePayload
                    | Command::ArchiveUpdateChunk
                    | Command::ArchiveUpdateAck
            )
        {
            return Err(ProtocolError::UnsupportedVersion(protocol::LEGACY_VERSION));
        }
        if matches!(
            command,
            Command::CompositePayload | Command::CompositeChunk | Command::CompositeAck
        ) && !self.negotiated_capabilities.composite_clipboard
        {
            return Err(ProtocolError::AdmissionRejected(
                "Composite clipboard was not negotiated".into(),
            ));
        }
        if matches!(
            command,
            Command::ArchiveUpdatePayload | Command::ArchiveUpdateChunk | Command::ArchiveUpdateAck
        ) && (!self.negotiated_capabilities.archive_resource_updates
            || !self.negotiated_capabilities.composite_clipboard)
        {
            return Err(ProtocolError::AdmissionRejected(
                "Archive resource updates were not negotiated".into(),
            ));
        }
        let payload_length = u32::from_be_bytes([
            self.read_buffer[12],
            self.read_buffer[13],
            self.read_buffer[14],
            self.read_buffer[15],
        ]) as usize;
        let limit = command.payload_limit();
        if payload_length > limit {
            return Err(ProtocolError::CommandPayloadTooLarge {
                command,
                actual: payload_length,
                limit,
            });
        }
        let total_size = protocol::HEADER_SIZE
            .checked_add(payload_length)
            .and_then(|size| size.checked_add(protocol::CHECKSUM_SIZE))
            .ok_or(ProtocolError::PayloadTooLarge(payload_length))?;
        Ok(Some((command, payload_length, total_size)))
    }

    async fn read_transport_record(&mut self) -> Result<(), ProtocolError> {
        while self.partial_header_len < self.partial_header.len() {
            let read = self
                .stream
                .read(&mut self.partial_header[self.partial_header_len..])
                .await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "early eof while reading encrypted record length",
                )
                .into());
            }
            self.partial_header_len += read;
        }

        if self.partial_expected.is_none() {
            let encrypted_length = u16::from_be_bytes(self.partial_header) as usize;
            if !(16..=MAX_TRANSPORT_RECORD).contains(&encrypted_length) {
                return Err(ProtocolError::InvalidEncryptedRecord);
            }
            self.partial_record.clear();
            self.partial_record.reserve(encrypted_length);
            self.partial_expected = Some(encrypted_length);
        }

        let expected = self
            .partial_expected
            .expect("encrypted record length set above");
        let mut chunk = [0u8; 8192];
        while self.partial_record.len() < expected {
            let remaining = expected - self.partial_record.len();
            let chunk_len = remaining.min(chunk.len());
            let read = self.stream.read(&mut chunk[..chunk_len]).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "early eof while reading encrypted record",
                )
                .into());
            }
            self.partial_record.extend_from_slice(&chunk[..read]);
        }

        let encrypted = std::mem::take(&mut self.partial_record);
        self.partial_expected = None;
        self.partial_header_len = 0;
        // Decrypt directly into the receive buffer's newly reserved tail.
        // The previous implementation allocated a second plaintext Vec and
        // copied it into read_buffer, doubling the peak plaintext footprint
        // for every encrypted record.
        let buffer_start = self.read_buffer.len();
        self.read_buffer
            .resize(buffer_start.saturating_add(encrypted.len()), 0);
        let length = match self
            .transport
            .read_message(&encrypted, &mut self.read_buffer[buffer_start..])
        {
            Ok(length) => length,
            Err(error) => {
                self.read_buffer[buffer_start..].fill(0);
                self.read_buffer.truncate(buffer_start);
                return Err(ProtocolError::TransportEncryption(error.to_string()));
            }
        };
        self.read_buffer.truncate(buffer_start + length);
        Ok(())
    }
}

mod handshake;

use handshake::{flush_with_timeout, write_all_with_timeout};

pub use handshake::{
    accept, accept_with_pairing_window, accept_with_pairing_window_and_capabilities, connect,
    connect_pairing, connect_pairing_with_capabilities, connect_with_capabilities, write_error,
    write_image_chunk_error, write_ready,
};

#[cfg(test)]
use handshake::{
    accept_with_test_capabilities, build_handshake, connect_with_test_capabilities,
    read_plain_frame,
};

pub fn decode_trusted_key(encoded: &str) -> Result<Vec<u8>, String> {
    identity::decode_public_key(encoded).map_err(|error| error.to_string())
}

pub fn fingerprint(public_key: &[u8]) -> String {
    identity::fingerprint(public_key)
}

#[cfg(test)]
mod tests;
