import { describe, expect, it } from "vitest";
import type { PeerDevice, PeerRoute } from "../tailsyncClient";
import { pairingAddressForPeer } from "./pairingAddress";

function route(overrides: Partial<PeerRoute>): PeerRoute {
  return {
    interface: "lan",
    address: "192.168.1.10",
    status: "online",
    online: true,
    connected: false,
    ...overrides,
  };
}

function peer(routes: PeerRoute[]): PeerDevice {
  return {
    hostname: "MacBook",
    tailscale_ip: "100.64.0.5",
    address: "192.168.1.10",
    online: true,
    enabled: true,
    connection_mode: "auto",
    trusted: false,
    fingerprint: "abcd",
    routes,
  };
}

describe("pairingAddressForPeer", () => {
  it("prefers a connected TCP route over an available Iroh route", () => {
    expect(pairingAddressForPeer(peer([
      route({ interface: "iroh", address: "5866666666666666666666666666666666666666666666666666666666666666" }),
      route({ interface: "lan", address: "192.168.1.10", connected: true }),
    ]))).toBe("192.168.1.10");
  });

  it("uses an online TCP route before falling back to Iroh", () => {
    expect(pairingAddressForPeer(peer([
      route({ interface: "iroh", address: "5866666666666666666666666666666666666666666666666666666666666666" }),
      route({ interface: "tailscale", address: "100.64.0.5", status: "online" }),
    ]))).toBe("100.64.0.5");
  });

  it("falls back to Iroh when no TCP route is available", () => {
    expect(pairingAddressForPeer(peer([
      route({ interface: "iroh", address: "5866666666666666666666666666666666666666666666666666666666666666" }),
    ]))).toBe("5866666666666666666666666666666666666666666666666666666666666666");
  });

  it("does not select an offline TCP route over an Iroh fallback", () => {
    expect(pairingAddressForPeer(peer([
      route({ interface: "lan", address: "192.168.1.10", status: "offline", online: false }),
      route({ interface: "iroh", address: "5866666666666666666666666666666666666666666666666666666666666666", status: "online" }),
    ]))).toBe("5866666666666666666666666666666666666666666666666666666666666666");
  });
});
