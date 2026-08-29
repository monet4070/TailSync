import type { PeerDevice, PeerRoute } from "../../tailsyncClient";

export const routeInterfaceLabel = (routeInterface: PeerRoute["interface"]) => {
  if (routeInterface === "lan") return "LAN";
  if (routeInterface === "iroh") return "Iroh";
  return "Tailscale";
};

export const peerCanSync = (peer: PeerDevice) =>
  peer.trusted && peer.enabled && (
    Boolean(peer.current_interface)
    || peer.online
    || Boolean(peer.address)
    || Boolean(peer.tailscale_ip)
    || Boolean(peer.routes?.some((route) => Boolean(route.address)))
  );

export const GIB = 1024 * 1024 * 1024;

export function formatStorageSize(bytes: number) {
  return `${(bytes / GIB).toFixed(bytes >= GIB ? 1 : 2)} GiB`;
}
