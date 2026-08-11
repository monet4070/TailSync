use std::io;
use std::path::Path;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId, SecretKey};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::db;
use crate::identity::{
    persist_protected_bytes_create_only, read_protected_bytes, CreateOutcome, IdentityError,
};

pub const ALPN: &[u8] = b"tailsync/3";
const SECRET_KEY_SIZE: usize = 32;

pub fn canonical_endpoint_id(value: &str) -> Result<String, String> {
    EndpointId::from_str(value.trim())
        .map(|endpoint_id| endpoint_id.to_string())
        .map_err(|error| format!("Invalid Iroh endpoint ID: {error}"))
}

pub fn persistent_endpoint_id() -> Result<String, String> {
    load_or_create_secret_key(&identity_path())
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
    _connection: Connection,
}

impl IrohEndpoint {
    pub async fn bind() -> Result<Self, String> {
        let secret_key = load_or_create_secret_key(&identity_path())
            .map_err(|error| format!("Could not load Iroh identity: {error}"))?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
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
        let connection = self
            .endpoint
            .connect(endpoint_id, ALPN)
            .await
            .map_err(|error| format!("Iroh connection failed: {error}"))?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|error| format!("Could not open Iroh stream: {error}"))?;
        Ok(IrohBiStream::new(send, recv, connection))
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
            _connection: connection,
        }
    }
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
        AsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx)
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
}
