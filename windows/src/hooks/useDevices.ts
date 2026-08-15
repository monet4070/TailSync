// Devices list feature hook (T259 extraction from Settings.tsx).
//
// Owns the device snapshot state, the initial load plus the 5s polling and
// peer-health-changed subscription (both active while the connection mode
// is known), the manual refresh with loading flag, the silent reload used
// after connection tests, and the optimistic peer toggle. The settings-hub
// patch after a successful toggle is injected so the feature stays
// independent of the settings page state.

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getPeers,
  refreshPeers,
  togglePeer,
  type PeerDevice,
  type PeersResponse,
} from "../tailsyncClient";
import type { SettingsData } from "../types/settings.generated";

export interface UseDevicesOptions {
  connectionMode: SettingsData["connection_mode"] | undefined;
  applyPeerEnabled: (hostname: string, enabled: boolean) => void;
}

export function useDevices(options: UseDevicesOptions) {
  const { connectionMode, applyPeerEnabled } = options;
  const [devices, setDevices] = useState<PeersResponse | null>(null);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [devicesError, setDevicesError] = useState("");

  const onDevicesRefreshed = useCallback(async () => {
    const result = await getPeers();
    setDevices(result);
    setDevicesError(result.discovery_error ?? "");
  }, []);

  const refreshDevices = useCallback(async () => {
    setDevicesLoading(true);
    try {
      const result = await refreshPeers();
      setDevices(result);
      setDevicesError(result.discovery_error ?? "");
    } catch (error) {
      setDevices(null);
      setDevicesError(String(error));
    } finally {
      setDevicesLoading(false);
    }
  }, []);

  const resetDevices = useCallback(() => {
    setDevices(null);
    setDevicesError("");
  }, []);

  const handlePeerToggle = useCallback(async (peer: PeerDevice, enabled: boolean) => {
    setDevices((current) => current ? {
      ...current,
      peers: current.peers.map((item) =>
        item.hostname === peer.hostname ? { ...item, enabled } : item,
      ),
    } : current);
    try {
      await togglePeer(peer.hostname, enabled);
      applyPeerEnabled(peer.hostname, enabled);
    } catch (error) {
      console.error("Toggle peer failed:", error);
      await refreshDevices();
    }
  }, [applyPeerEnabled, refreshDevices]);

  useEffect(() => {
    if (!connectionMode) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    const load = async (showLoading = false) => {
      if (showLoading) setDevicesLoading(true);
      try {
        const result = await getPeers();
        if (active) {
          setDevices(result);
          setDevicesError(result.discovery_error ?? "");
        }
      } catch (error) {
        if (active) {
          setDevices(null);
          setDevicesError(String(error));
        }
      } finally {
        if (active && showLoading) setDevicesLoading(false);
      }
    };
    void load(true);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void load();
    }, 5000);
    void listen("peer-health-changed", () => void load()).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      window.clearInterval(timer);
      unlisten?.();
    };
  }, [connectionMode]);

  return {
    devices,
    devicesLoading,
    devicesError,
    refreshDevices,
    onDevicesRefreshed,
    resetDevices,
    handlePeerToggle,
  };
}
