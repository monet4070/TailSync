// Pure pairing-address resolution (T258 extraction from Settings.tsx).
//
// Picks the best route address for initiating a pairing handshake with a
// peer: an iroh route wins outright; otherwise the connected/online/
// confirming route is preferred, falling back to the peer's own address
// fields. An iroh-only peer with no iroh route cannot be paired over TCP.

import type { PeerDevice } from "../tailsyncClient";

export function pairingAddressForPeer(peer: PeerDevice): string | null {
  const routes = (peer.routes ?? []).filter((route) => route.address.trim());
  const irohRoute = routes.find((candidate) => candidate.interface === "iroh");
  if (irohRoute) return irohRoute.address;
  if (peer.current_interface === "iroh") return null;
  const route = routes.find((candidate) => candidate.connected)
    ?? routes.find((candidate) => candidate.online)
    ?? routes.find((candidate) => candidate.status === "confirming")
    ?? routes.find((candidate) => candidate.address === peer.current_address)
    ?? routes.find((candidate) => candidate.address === peer.address)
    ?? routes[0];
  return route?.address ?? (peer.address?.trim() || peer.tailscale_ip?.trim() || null);
}
