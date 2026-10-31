//! Read-only TCP/Noise/heartbeat probe with an existing identity and pinned key.
//! Set TAILSYNC_DATA_DIR to an isolated copy of config-v2.json, identity-v1.bin,
//! and its .dek encryption key file (protected for the same Windows account).
//! Usage: peer_connectivity <peer-hostname> <ip:port>
use std::time::{Duration, Instant};
use tailsync_core::{
    crypto::Settings,
    db,
    identity::DeviceIdentity,
    protocol::{Command, Frame},
    secure,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> Result {
    let mut args = std::env::args().skip(1);
    let peer = args.next().ok_or("missing paired hostname")?;
    let address = args.next().ok_or("missing IP and port")?;
    let root = db::get_data_dir();
    if std::env::var_os("TAILSYNC_DATA_DIR").is_none() || !root.join("identity-v1.bin").is_file() {
        return Err("Probe requires an explicit profile copy with an existing identity".into());
    }
    let settings: Settings = serde_json::from_slice(&std::fs::read(root.join("config-v2.json"))?)?;
    let key = secure::decode_trusted_key(
        settings
            .trusted_peer_keys
            .get(&peer)
            .ok_or("peer is not paired")?,
    )?;
    let identity = DeviceIdentity::load_or_create().map_err(|error| error.to_string())?;
    let started = Instant::now();
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::TcpStream::connect(&address),
    )
    .await??;
    println!("TCP_CONNECTED elapsed_ms={}", started.elapsed().as_millis());
    let mut connection = tokio::time::timeout(
        Duration::from_secs(5),
        secure::connect(
            stream,
            &identity,
            secure::PeerIdentity {
                hostname: std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"))?,
                tailscale_ip: String::new(),
                iroh_endpoint_id: None,
            },
            &peer,
            &key,
        ),
    )
    .await??;
    println!(
        "NOISE_AUTHENTICATED elapsed_ms={}",
        started.elapsed().as_millis()
    );
    connection
        .write_frame(&Frame::try_new(Command::Heartbeat, 0, 1, Vec::new())?)
        .await?;
    let reply = tokio::time::timeout(Duration::from_secs(5), connection.read_frame()).await??;
    if reply.command != Command::HeartbeatAck {
        return Err("peer did not acknowledge the authenticated heartbeat".into());
    }
    println!("HEARTBEAT_ACK elapsed_ms={}", started.elapsed().as_millis());
    connection.shutdown().await?;
    Ok(())
}
