// Connection test state and actions (T255 extraction from Settings.tsx).
//
// Runs a route latency test through the typed client, tracks per-route
// status, and refreshes the device snapshot afterwards. The refresh
// callback is injected so the feature stays independent of the devices
// state; errors from it are swallowed to keep the test result visible.

import { useCallback, useState } from "react";
import { testConnection, type PeerDevice, type PeerRoute } from "../tailsyncClient";

export interface ConnectionTestState {
  status: "testing" | "success" | "error";
  latency_ms?: number;
  path?: "tcp" | "direct" | "relay";
}

export function useConnectionTests(onDevicesRefreshed: () => Promise<void>) {
  const [connectionTests, setConnectionTests] = useState<Record<string, ConnectionTestState>>({});

  const handleTestConnection = useCallback(
    async (peer: PeerDevice, route: PeerRoute) => {
      const key = `${peer.hostname}|${route.interface}|${route.address}`;
      setConnectionTests((current) => ({
        ...current,
        [key]: { status: "testing" },
      }));
      try {
        const result = await testConnection(route.address);
        setConnectionTests((current) => ({
          ...current,
          [key]: { status: "success", latency_ms: result.latency_ms, path: result.path },
        }));
      } catch (error) {
        console.error("Connection test failed:", error);
        setConnectionTests((current) => ({
          ...current,
          [key]: { status: "error" },
        }));
      } finally {
        try {
          await onDevicesRefreshed();
        } catch {
          // Keep the explicit connection-test result visible.
        }
      }
    },
    [onDevicesRefreshed],
  );

  return { connectionTests, handleTestConnection };
}
