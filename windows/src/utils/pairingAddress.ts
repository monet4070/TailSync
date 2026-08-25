// Pure pairing-address resolution (T258 extraction from Settings.tsx).
//
// Picks the safest available route address for initiating a pairing handshake.
// TCP is preferred for pairing because it has a graceful stream close and is
// easier to diagnose; Iroh remains the fallback when no TCP route exists.

import type { PeerDevice } from "../tailsyncClient";

export function pairingAddressForPeer(peer: PeerDevice): string | null {
  const routes = (peer.routes ?? []).filter((route) => route.address.trim());
  const tcpRoutes = routes.filter((candidate) => candidate.interface !== "iroh");
  const activeTcpRoute = tcpRoutes.find((candidate) => candidate.connected)
    ?? tcpRoutes.find((candidate) => candidate.online)
    ?? tcpRoutes.find((candidate) => candidate.status === "confirming");
  const irohRoute = routes.find((candidate) => candidate.interface === "iroh");
  const route = activeTcpRoute
    ?? irohRoute
    ?? tcpRoutes.find((candidate) => candidate.address === peer.current_address)
    ?? tcpRoutes.find((candidate) => candidate.address === peer.address)
    ?? tcpRoutes[0]
    ?? routes[0];
  return route?.address ?? (peer.address?.trim() || peer.tailscale_ip?.trim() || null);
}
