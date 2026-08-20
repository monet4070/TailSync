//! Normalized view of an inbound peer connection source.
//!
//! Shared by the macOS and Windows network layers (T104 migration). An
//! inbound connection arrives either over TCP (with a socket address) or
//! over Iroh (with a remote endpoint id); this type centralizes the
//! interface/address/description mapping and the connection-mode check that
//! both server paths apply.

use crate::peer::directory::{infer_interface, source_matches_mode};
use crate::peer::types::ConnectionInterface;
use std::net::SocketAddr;

/// Where an inbound peer connection came from.
pub enum InboundSource {
    Tcp(SocketAddr),
    Iroh(String),
}

impl InboundSource {
    /// The connection interface for this source.
    pub fn interface(&self) -> Result<ConnectionInterface, String> {
        match self {
            Self::Tcp(address) => infer_interface(&address.ip().to_string()),
            Self::Iroh(_) => Ok(ConnectionInterface::Iroh),
        }
    }

    /// The routable address of this source (IP for TCP, endpoint id for Iroh).
    pub fn address(&self) -> String {
        match self {
            Self::Tcp(address) => address.ip().to_string(),
            Self::Iroh(endpoint_id) => endpoint_id.clone(),
        }
    }

    /// A human-readable description of this source.
    pub fn description(&self) -> String {
        match self {
            Self::Tcp(address) => address.to_string(),
            Self::Iroh(endpoint_id) => format!("iroh:{endpoint_id}"),
        }
    }

    /// Whether this source is allowed under the given connection mode.
    pub fn is_allowed(&self, mode: &str) -> bool {
        match self {
            Self::Tcp(address) => source_matches_mode(address.ip(), mode),
            Self::Iroh(_) => mode == "auto",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp(address: &str) -> InboundSource {
        InboundSource::Tcp(address.parse().unwrap())
    }

    #[test]
    fn interface_maps_tcp_addresses_and_iroh() {
        assert_eq!(
            tcp("192.168.1.5:19890").interface(),
            Ok(ConnectionInterface::Lan)
        );
        assert_eq!(
            tcp("100.101.102.103:19890").interface(),
            Ok(ConnectionInterface::Tailscale)
        );
        assert_eq!(
            InboundSource::Iroh("endpoint-1".to_string()).interface(),
            Ok(ConnectionInterface::Iroh)
        );
    }

    #[test]
    fn address_and_description_normalize_the_source() {
        let source = tcp("192.168.1.5:19890");
        assert_eq!(source.address(), "192.168.1.5");
        assert_eq!(source.description(), "192.168.1.5:19890");

        let iroh = InboundSource::Iroh("endpoint-1".to_string());
        assert_eq!(iroh.address(), "endpoint-1");
        assert_eq!(iroh.description(), "iroh:endpoint-1");
    }

    #[test]
    fn is_allowed_follows_the_connection_mode() {
        let lan = tcp("192.168.1.5:19890");
        assert!(lan.is_allowed("lan_only"));
        assert!(lan.is_allowed("auto"));
        assert!(!lan.is_allowed("tailscale_only"));

        let tailscale = tcp("100.101.102.103:19890");
        assert!(tailscale.is_allowed("tailscale_only"));
        assert!(!tailscale.is_allowed("lan_only"));

        let iroh = InboundSource::Iroh("endpoint-1".to_string());
        assert!(iroh.is_allowed("auto"));
        assert!(!iroh.is_allowed("lan_only"));
        assert!(!iroh.is_allowed("tailscale_only"));
    }
}
