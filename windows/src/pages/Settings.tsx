import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import {
  COLOR_THEMES,
  isColorTheme,
  isThemePreference,
  useTheme,
  type ColorTheme,
  type ThemePreference,
} from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { LatestRequest, SerialTaskQueue } from "../utils/asyncControl";
import type { SettingsData } from "../types/settings.generated";
import {
  Activity,
  Check,
  ChevronDown,
  Grid2X2,
  FolderOpen,
  HardDrive,
  Monitor,
  Moon,
  RefreshCw,
  Settings2,
  Sun,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import tailsyncIcon from "../../src-tauri/icons/32x32.png";

/* ── Types ──────────────────────────────────────────────────────── */

interface PeerDevice {
  hostname: string;
  tailscale_ip: string;
  address: string;
  online: boolean;
  enabled: boolean;
  connection_mode: "auto" | "lan" | "tailscale";
  trusted: boolean;
  fingerprint: string;
  current_interface?: "lan" | "tailscale";
  current_address?: string | null;
  status?: "discovered" | "online" | "confirming" | "offline" | "connected";
  routes?: PeerRoute[];
}

interface PeerRoute {
  interface: "lan" | "tailscale";
  address: string;
  status: "discovered" | "online" | "confirming" | "offline" | "connected";
  online: boolean;
  connected: boolean;
  latency_ms?: number | null;
  pairing_endpoint?: boolean;
}

interface ConnectionTestResult {
  latency_ms: number;
}

interface ConnectionTestState {
  status: "testing" | "success" | "error";
  latency_ms?: number;
}

interface PeersResponse {
  self: {
    hostname: string;
    tailscale_ip: string;
    connection_mode: "auto" | "lan_only" | "tailscale_only";
    public_key: string;
    fingerprint: string;
    routes?: PeerRoute[];
  };
  peers: PeerDevice[];
  paired_peer_endpoints: Record<string, string>;
  discovery_error?: string | null;
}

interface PairingPeerStatus {
  hostname: string;
  address: string;
  fingerprint: string;
  verification_code: string;
  local_confirmed: boolean;
  remote_confirmed: boolean;
}

interface PairingStatus {
  pairing_enabled: boolean;
  phase: "disabled" | "waiting" | "handshaking" | "verification" |
    "waiting_for_peer" | "paired" | "cancelled" | "timed_out" | "locked";
  expires_at?: number | null;
  remaining_seconds: number;
  failed_attempts: number;
  max_failures: number;
  peer?: PairingPeerStatus | null;
  error?: string | null;
}

interface StorageStatus {
  root: string;
  used_bytes: number;
  quota_bytes: number;
  available: boolean;
  error?: string | null;
}

interface StorageMigrationResult {
  new_root: string;
  old_root: string;
  old_size_bytes: number;
}

const GIB = 1024 * 1024 * 1024;

function formatStorageSize(bytes: number) {
  return `${(bytes / GIB).toFixed(bytes >= GIB ? 1 : 2)} GiB`;
}

/* ── Component ──────────────────────────────────────────────────── */

export function Settings() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [historyLimitDraft, setHistoryLimitDraft] = useState(100);
  const [storageQuotaDraft, setStorageQuotaDraft] = useState("10");
  const [storageStatus, setStorageStatus] = useState<StorageStatus | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [oldStorage, setOldStorage] = useState<StorageMigrationResult | null>(null);
  const [saved, setSaved] = useState(false);
  const [devices, setDevices] = useState<PeersResponse | null>(null);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [devicesError, setDevicesError] = useState("");
  const [pairingTarget, setPairingTarget] = useState<PeerDevice | null>(null);
  const [pairingStatus, setPairingStatus] = useState<PairingStatus | null>(null);
  const [pairingOpen, setPairingOpen] = useState(false);
  const [pairingError, setPairingError] = useState("");
  const [pairingBusy, setPairingBusy] = useState(false);
  const [connectionTests, setConnectionTests] = useState<Record<string, ConnectionTestState>>({});
  const {
    theme,
    setTheme,
    themePreference,
    colorTheme,
    setColorTheme,
  } = useTheme();
  const { t, setLocale } = useI18n();
  const toastTimer = useRef<number>(0);
  const previousPairingPhase = useRef<PairingStatus["phase"] | null>(null);
  const pairingBusyRef = useRef(pairingBusy);
  const settingsRef = useRef<SettingsData | null>(null);
  const saveQueue = useRef(new SerialTaskQueue());
  const settingsUpdates = useRef(new LatestRequest());
  const pairDialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((s) => {
        settingsRef.current = s;
        setSettings(s);
        setHistoryLimitDraft(s.history_limit);
        setStorageQuotaDraft(String(Math.round(s.storage_quota_bytes / GIB)));
        if (isThemePreference(s.theme)) setTheme(s.theme);
        if (isColorTheme(s.color_theme)) setColorTheme(s.color_theme);
        setLocale(s.language);
      })
      .catch(console.error);
    invoke<StorageStatus>("get_storage_status").then(setStorageStatus).catch(console.error);
  }, [setColorTheme, setLocale, setTheme]);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);
  useEffect(() => {
    pairingBusyRef.current = pairingBusy;
  }, [pairingBusy]);

  const connectionMode = settings?.connection_mode;
  useEffect(() => {
    if (!connectionMode) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    const load = async (showLoading = false) => {
      if (showLoading) setDevicesLoading(true);
      try {
        const result = await invoke<PeersResponse>("get_peers");
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
    const timer = window.setInterval(() => void load(), 5000);
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

  const update = async (patch: Partial<SettingsData>) => {
    const previous = settingsRef.current;
    if (!previous) return false;
    const next = { ...previous, ...patch };
    settingsRef.current = next;
    setSettings(next);
    const generation = settingsUpdates.current.begin();
    const save = saveQueue.current.enqueue(() =>
      invoke("update_settings", { settingsJson: JSON.stringify(next) }),
    );
    try {
      await save;
      if (settingsUpdates.current.isCurrent(generation)) {
        setSaved(true);
        window.clearTimeout(toastTimer.current);
        toastTimer.current = window.setTimeout(() => setSaved(false), 1500);
      }
      return true;
    } catch (e) {
      if (settingsUpdates.current.isCurrent(generation)) {
        try {
          const canonical = await invoke<SettingsData>("get_settings");
          settingsRef.current = canonical;
          setSettings(canonical);
          setHistoryLimitDraft(canonical.history_limit);
        } catch {
          settingsRef.current = previous;
          setSettings(previous);
          setHistoryLimitDraft(previous.history_limit);
        }
      }
      console.error("Save settings failed:", e);
      return false;
    }
  };

  const commitHistoryLimit = async () => {
    if (historyLimitDraft === settingsRef.current?.history_limit) return;
    await update({ history_limit: historyLimitDraft });
  };

  const commitStorageQuota = async () => {
    const current = settingsRef.current;
    if (!current) return;
    const currentGib = Math.round(current.storage_quota_bytes / GIB);
    const parsed = Number.parseInt(storageQuotaDraft, 10);
    const gib = Math.min(16384, Math.max(1, Number.isFinite(parsed) ? parsed : currentGib));
    setStorageQuotaDraft(String(gib));
    if (gib === currentGib) return;
    if (!(await update({ storage_quota_bytes: gib * GIB }))) {
      setStorageQuotaDraft(
        String(Math.round((settingsRef.current?.storage_quota_bytes ?? current.storage_quota_bytes) / GIB)),
      );
    }
  };

  const changeStorage = async () => {
    const parent = await open({
      directory: true,
      multiple: false,
      title: t("settings.storageChoose"),
    });
    if (typeof parent !== "string") return;
    setStorageBusy(true);
    try {
      const result = await invoke<StorageMigrationResult>("change_storage_location", { parent });
      setOldStorage(result);
      const [canonical, status] = await Promise.all([
        invoke<SettingsData>("get_settings"),
        invoke<StorageStatus>("get_storage_status"),
      ]);
      settingsRef.current = canonical;
      setSettings(canonical);
      setStorageQuotaDraft(String(Math.round(canonical.storage_quota_bytes / GIB)));
      setStorageStatus(status);
    } catch (error) {
      console.error("Storage migration failed:", error);
    } finally {
      setStorageBusy(false);
    }
  };

  const deleteOldStorage = async () => {
    if (!oldStorage) return;
    await invoke("delete_old_storage", { path: oldStorage.old_root });
    setOldStorage(null);
  };

  useEffect(() => {
    if (!pairingOpen) return;
    const dialog = pairDialogRef.current;
    if (!dialog) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    const focusableSelector = [
      "button:not([disabled])",
      "[href]",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");
    const focusFirst = () => {
      const focusable = dialog.querySelectorAll<HTMLElement>(focusableSelector);
      (focusable[0] ?? dialog).focus();
    };
    const frame = window.requestAnimationFrame(focusFirst);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void closePairing();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...dialog.querySelectorAll<HTMLElement>(focusableSelector)];
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus.current?.focus();
      previousFocus.current = null;
    };
  }, [pairingOpen]);

  const refreshDevices = async () => {
    setDevicesLoading(true);
    try {
      const result = await invoke<PeersResponse>("refresh_peers");
      setDevices(result);
      setDevicesError(result.discovery_error ?? "");
    } catch (error) {
      setDevices(null);
      setDevicesError(String(error));
    } finally {
      setDevicesLoading(false);
    }
  };

  const handleConnectionMode = async (mode: "auto" | "lan_only" | "tailscale_only") => {
    if (mode === settings?.connection_mode) return;
    setDevices(null);
    setDevicesError("");
    if (await update({ connection_mode: mode })) {
      await refreshDevices();
    }
  };

  const handlePeerToggle = async (peer: PeerDevice, enabled: boolean) => {
    setDevices((current) => current ? {
      ...current,
      peers: current.peers.map((item) =>
        item.hostname === peer.hostname ? { ...item, enabled } : item,
      ),
    } : current);
    try {
      await invoke("toggle_peer", { hostname: peer.hostname, enabled });
      setSettings((current) => current ? {
        ...current,
        enabled_peers: { ...current.enabled_peers, [peer.hostname]: enabled },
      } : current);
    } catch (error) {
      console.error("Toggle peer failed:", error);
      await refreshDevices();
    }
  };

  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const status = await invoke<PairingStatus>("get_pairing_status");
        if (!active) return;
        setPairingStatus(status);
        if (status.peer && ["verification", "waiting_for_peer"].includes(status.phase)) {
          setPairingOpen(true);
        }
        if (status.phase === "paired" && previousPairingPhase.current !== "paired") {
          setPairingOpen(false);
          setPairingTarget(null);
          void refreshDevices();
        }
        previousPairingPhase.current = status.phase;
      } catch (error) {
        if (active) setPairingError(String(error));
      }
    };
    void poll();
    const timer = window.setInterval(poll, 1000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  const enablePairing = async () => {
    setPairingBusy(true);
    setPairingError("");
    try {
      const status = await invoke<PairingStatus>("enable_pairing");
      setPairingStatus(status);
      setPairingOpen(true);
    } catch (error) {
      setPairingError(String(error));
    } finally {
      setPairingBusy(false);
    }
  };

  const openPairing = async (peer: PeerDevice) => {
    const address = peer.address || peer.tailscale_ip;
    if (!address) return;
    setPairingTarget(peer);
    setPairingOpen(true);
    setPairingBusy(true);
    setPairingError("");
    try {
      if (!pairingStatus?.pairing_enabled) {
        setPairingStatus(await invoke<PairingStatus>("enable_pairing"));
      }
      const status = await invoke<PairingStatus>("start_pairing", { address });
      setPairingStatus(status);
    } catch (error) {
      setPairingError(String(error));
      try {
        setPairingStatus(await invoke<PairingStatus>("get_pairing_status"));
      } catch {
        // Preserve the original pairing error.
      }
    } finally {
      setPairingBusy(false);
    }
  };

  const closePairing = async () => {
    if (pairingBusyRef.current) return;
    pairingBusyRef.current = true;
    setPairingBusy(true);
    try {
      setPairingStatus(await invoke<PairingStatus>("cancel_pairing"));
      setPairingOpen(false);
      setPairingTarget(null);
    } catch (error) {
      setPairingError(String(error));
    } finally {
      pairingBusyRef.current = false;
      setPairingBusy(false);
    }
  };

  const handlePair = async () => {
    if (!pairingStatus?.peer) return;
    setPairingBusy(true);
    setPairingError("");
    try {
      setPairingStatus(await invoke<PairingStatus>("confirm_pairing"));
    } catch (error) {
      setPairingError(String(error));
    } finally {
      setPairingBusy(false);
    }
  };

  const handleForget = async (peer: PeerDevice) => {
    try {
      await invoke("forget_peer", { hostname: peer.hostname });
      await refreshDevices();
    } catch (error) {
      console.error("Forget peer failed:", error);
    }
  };

  const testConnection = async (peer: PeerDevice, route: PeerRoute) => {
    const key = `${peer.hostname}|${route.interface}|${route.address}`;
    setConnectionTests((current) => ({
      ...current,
      [key]: { status: "testing" },
    }));
    try {
      const result = await invoke<ConnectionTestResult>("test_connection", {
        address: route.address,
      });
      setConnectionTests((current) => ({
        ...current,
        [key]: { status: "success", latency_ms: result.latency_ms },
      }));
    } catch (error) {
      console.error("Connection test failed:", error);
      setConnectionTests((current) => ({
        ...current,
        [key]: { status: "error" },
      }));
    } finally {
      try {
        const result = await invoke<PeersResponse>("get_peers");
        setDevices(result);
        setDevicesError(result.discovery_error ?? "");
      } catch {
        // Keep the explicit connection-test result visible.
      }
    }
  };

  const handleThemeChange = async (value: ThemePreference) => {
    if (value === themePreference) return;
    const previous = themePreference;
    setTheme(value);
    if (!(await update({ theme: value }))) setTheme(previous);
  };

  const handleColorThemeChange = async (value: ColorTheme) => {
    if (value === colorTheme) return;
    const previous = colorTheme;
    setColorTheme(value);
    if (!(await update({ color_theme: value }))) setColorTheme(previous);
  };

  const appClassName = `app ${theme} theme-${colorTheme}`;

  if (!settings) {
    return (
      <div className={appClassName}>
        {/* Title bar */}
        <div className="titlebar" data-tauri-drag-region>
          <div className="titlebar-brand">
            <div className="titlebar-logo">
              <img src={tailsyncIcon} alt="" />
            </div>
            <span className="titlebar-text">{t("settings.title")}</span>
            <span className="titlebar-badge">v2</span>
          </div>
          <button
            className="titlebar-close"
            onClick={() => getCurrentWindow().hide()}
            title={t("settings.closePairing")}
            aria-label={t("settings.closePairing")}
          >
            <X size={15} strokeWidth={1.8} aria-hidden="true" />
          </button>
        </div>
        <div className="loading-text">{t("settings.loading")}</div>
      </div>
    );
  }

  return (
    <div className={appClassName}>
      {/* ── Title bar ── */}
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <div className="titlebar-logo">
            <img src={tailsyncIcon} alt="" />
          </div>
          <span className="titlebar-text">{t("settings.title")}</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => getCurrentWindow().hide()}
          title={t("settings.closePairing")}
          aria-label={t("settings.closePairing")}
        >
          <X size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>

      {/* ── Settings content ── */}
      <div className="settings-content">
        {/* Connections and devices */}
        <section className="setting-group connection-group">
          <div className="setting-group-header section-header-with-action">
            <div>
              <h3>{t("settings.connectionsTitle")}</h3>
              <p>{t("settings.connectionsDescription")}</p>
            </div>
            <button
              type="button"
              className="icon-button"
              onClick={refreshDevices}
              disabled={devicesLoading}
              title={t("settings.refreshDevices")}
              aria-label={t("settings.refreshDevices")}
            >
              <RefreshCw className={devicesLoading ? "spin" : ""} size={16} strokeWidth={1.7} aria-hidden="true" />
            </button>
          </div>

          <div className="connection-mode" role="radiogroup" aria-label={t("settings.connectionMode")}>
            <button
              type="button"
              className={settings.connection_mode === "auto" ? "active" : ""}
              onClick={() => handleConnectionMode("auto")}
              role="radio"
              aria-checked={settings.connection_mode === "auto"}
            >
              {t("settings.modeAuto")}
            </button>
            <button
              type="button"
              className={settings.connection_mode === "lan_only" ? "active" : ""}
              onClick={() => handleConnectionMode("lan_only")}
              role="radio"
              aria-checked={settings.connection_mode === "lan_only"}
            >
              <Wifi size={15} strokeWidth={1.7} aria-hidden="true" />
              {t("settings.modeLan")}
            </button>
            <button
              type="button"
              className={settings.connection_mode === "tailscale_only" ? "active" : ""}
              onClick={() => handleConnectionMode("tailscale_only")}
              role="radio"
              aria-checked={settings.connection_mode === "tailscale_only"}
            >
              <Grid2X2 size={15} strokeWidth={1.7} aria-hidden="true" />
              Tailscale
            </button>
          </div>

          <div className="pairing-window-row">
            <div>
              <strong>{t("settings.devicePairing")}</strong>
              <span>
                {pairingStatus?.pairing_enabled
                  ? `${t("settings.waiting")} · ${pairingStatus.remaining_seconds}s · ${pairingStatus.failed_attempts}/${pairingStatus.max_failures}`
                  : t("settings.pairingClosed")}
              </span>
            </div>
            <button
              type="button"
              className={pairingStatus?.pairing_enabled ? "pairing-window-close" : "pair-device-action"}
              disabled={pairingBusy}
              onClick={() => pairingStatus?.pairing_enabled ? void closePairing() : void enablePairing()}
            >
              {t(pairingStatus?.pairing_enabled
                ? "settings.closePairing"
                : "settings.allowPairing")}
            </button>
          </div>

          <div className="device-list" aria-live="polite">
            {devices && (
              <div className="device-row local-device">
                <div className="device-avatar self">{devices.self.hostname.slice(0, 1).toUpperCase()}</div>
                <div className="device-info">
                  <div className="device-name">
                    <span className="device-name-text">{devices.self.hostname}</span>
                    <span>{t("settings.thisDevice")}</span>
                  </div>
                  <div className="device-fingerprint">{devices.self.fingerprint}</div>
                  <div className="peer-route-list local-route-list">
                    {(devices.self.routes?.length
                      ? devices.self.routes
                      : devices.self.tailscale_ip
                        ? [{
                          interface: devices.self.connection_mode === "tailscale_only" ? "tailscale" : "lan",
                          address: devices.self.tailscale_ip,
                          status: "connected",
                          online: true,
                          connected: true,
                          latency_ms: null,
                        } satisfies PeerRoute]
                        : []).map((route) => (
                          <div className="peer-route" key={`${route.interface}-${route.address}`}>
                            <span className="peer-route-address">{route.address}</span>
                            <span className={`peer-route-interface ${route.interface}`}>
                              {route.interface === "lan" ? "LAN" : "Tailscale"}
                            </span>
                            <span className="peer-route-status positive">
                              {t("settings.online")}
                            </span>
                          </div>
                        ))}
                  </div>
                </div>
              </div>
            )}

            {devices?.peers.map((peer) => {
              const routes = peer.routes?.length
                ? peer.routes
                : (peer.address || peer.tailscale_ip)
                  ? [{
                    interface: peer.current_interface ?? (peer.connection_mode === "tailscale" ? "tailscale" : "lan"),
                    address: peer.address || peer.tailscale_ip,
                    status: peer.current_interface ? "connected" : peer.online ? "online" : "offline",
                    online: peer.online,
                    connected: Boolean(peer.current_interface),
                    latency_ms: null,
                  } satisfies PeerRoute]
                  : [];
              return (
                <div className="device-row peer-device-row" key={peer.hostname}>
                  <div className="device-avatar">{peer.hostname.slice(0, 1).toUpperCase()}</div>
                  <div className="device-info">
                    <div className="device-name">
                      <span className="device-name-text">{peer.hostname}</span>
                      <span className={peer.trusted ? "peer-badge paired" : "peer-badge unpaired"}>
                        {t(peer.trusted ? "settings.paired" : "settings.notPaired")}
                      </span>
                    </div>
                    <div className="device-fingerprint">
                      {peer.trusted ? peer.fingerprint : t("settings.waitingSecurePairing")}
                    </div>
                    {routes.length > 0 ? (
                      <div className="peer-route-list">
                        {routes.map((route) => {
                          const testKey = `${peer.hostname}|${route.interface}|${route.address}`;
                          const test = connectionTests[testKey];
                          const reachabilityStatus = route.status === "connected" ? "online" : route.status;
                          return (
                            <div className="peer-route" key={`${route.interface}-${route.address}`}>
                              <span className="peer-route-address">{route.address}</span>
                              <span className={`peer-route-interface ${route.interface}`}>
                                {route.interface === "lan" ? "LAN" : "Tailscale"}
                              </span>
                              <span className={`peer-route-status health-${reachabilityStatus}`}>
                                {reachabilityStatus === "online"
                                  ? `${t("settings.online")}${route.latency_ms != null ? ` · ${route.latency_ms} ms` : ""}`
                                  : reachabilityStatus === "confirming"
                                    ? t("settings.confirming")
                                    : reachabilityStatus === "discovered"
                                      ? t("settings.discovered")
                                      : t("settings.offline")}
                              </span>
                              <span className={`peer-route-connection ${route.connected ? "connected" : "idle"}`}>
                                {t(route.connected ? "settings.connected" : "settings.notConnected")}
                              </span>
                              <button
                                type="button"
                                className="connection-test-button"
                                disabled={test?.status === "testing"}
                                onClick={() => void testConnection(peer, route)}
                                title={t("settings.testTcpPort")}
                                aria-label={`${t("settings.testAddress")}: ${route.address}`}
                              >
                                {test?.status === "testing"
                                  ? <RefreshCw className="spin" size={16} strokeWidth={1.7} aria-hidden="true" />
                                  : <Activity size={16} strokeWidth={1.7} aria-hidden="true" />}
                              </button>
                              {test?.status === "success" && (
                                <span className="connection-test-result success">{test.latency_ms} ms</span>
                              )}
                              {test?.status === "error" && (
                                <span className="connection-test-result error">
                                  {t("settings.failed")}
                                </span>
                              )}
                            </div>
                          );
                        })}
                      </div>
                    ) : (
                      <div className="device-address">
                        {t("settings.pairedWaiting")}
                      </div>
                    )}
                  </div>
                  <div className="device-actions">
                    {peer.trusted ? (
                      <>
                        <label className="toggle" title={t(peer.enabled ? "settings.disableSync" : "settings.enableSync")}>
                          <input
                            type="checkbox"
                            checked={peer.enabled}
                            onChange={(event) => handlePeerToggle(peer, event.target.checked)}
                          />
                          <div className="toggle-track" />
                        </label>
                        <button
                          type="button"
                          className="icon-button"
                          onClick={() => handleForget(peer)}
                          title={t("settings.forgetPairing")}
                          aria-label={t("settings.forgetPairing")}
                        >
                          <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
                        </button>
                      </>
                    ) : (
                      <button type="button" className="pair-device-action" onClick={() => void openPairing(peer)}>
                        {t("settings.pair")}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}

            {devicesLoading && !devices && (
              <div className="device-list-state">{t("settings.discoveringDevices")}</div>
            )}
            {!devicesLoading && devices && devices.peers.length === 0 && (
              <div className="device-list-state">
                {t("settings.noDevices")}
              </div>
            )}
            {!devicesLoading && devicesError && (
              <div className="device-list-state error">
                {t(settings.connection_mode === "tailscale_only"
                  ? "settings.tailscaleUnavailable"
                  : "settings.lanUnavailable")}
              </div>
            )}
          </div>
        </section>

        {/* General */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.general")}</h3>
            <p>{t("settings.generalDescription")}</p>
          </div>

          <div
            className="setting-row"
            onClick={() =>
              update({
                notifications_enabled: !settings.notifications_enabled,
              })
            }
          >
            <div className="setting-row-info">
              <span>{t("settings.notifications")}</span>
              <small>{t("settings.notificationsDescription")}</small>
            </div>
            <label className="toggle" onClick={(e) => e.stopPropagation()}>
              <input
                type="checkbox"
                checked={settings.notifications_enabled}
                onChange={(e) =>
                  update({ notifications_enabled: e.target.checked })
                }
              />
              <div className="toggle-track" />
            </label>
          </div>

          <div
            className="setting-row"
            onClick={() =>
              update({
                progress_bar_enabled: !settings.progress_bar_enabled,
              })
            }
          >
            <div className="setting-row-info">
              <span>{t("settings.progressBar")}</span>
              <small>{t("settings.progressDescription")}</small>
            </div>
            <label className="toggle" onClick={(e) => e.stopPropagation()}>
              <input
                type="checkbox"
                checked={settings.progress_bar_enabled}
                onChange={(e) =>
                  update({ progress_bar_enabled: e.target.checked })
                }
              />
              <div className="toggle-track" />
            </label>
          </div>
        </section>

        {/* History */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.history")}</h3>
            <p>{t("settings.historyDescription")}</p>
          </div>

          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.historyLimit")}</span>
              <small>
                {t("settings.historyLimitDescriptionPrefix")} {historyLimitDraft}{" "}
                {t("settings.historyLimitDescriptionSuffix")}
              </small>
            </div>
            <input
              type="range"
              min={10}
              max={500}
              value={historyLimitDraft}
              aria-label={t("settings.historyLimit")}
              onChange={(e) => setHistoryLimitDraft(Number(e.target.value))}
              onPointerUp={() => void commitHistoryLimit()}
              onBlur={() => void commitHistoryLimit()}
              onKeyUp={(event) => {
                if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
                  void commitHistoryLimit();
                }
              }}
            />
            <span className="range-value">{historyLimitDraft}</span>
          </div>
        </section>

        {/* Appearance */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.storage")}</h3>
            <p>{t("settings.storageDescription")}</p>
          </div>
          <div className="setting-row storage-row">
            <HardDrive size={17} strokeWidth={1.7} aria-hidden="true" />
            <div className="setting-row-info storage-location">
              <span title={storageStatus?.root}>{storageStatus?.root ?? settings.storage_root ?? ""}</span>
              <small>
                {storageStatus?.available === false
                  ? storageStatus.error
                  : `${formatStorageSize(storageStatus?.used_bytes ?? 0)} / ${formatStorageSize(settings.storage_quota_bytes)}`}
              </small>
            </div>
            <button className="storage-change" type="button" onClick={() => void changeStorage()} disabled={storageBusy}>
              <FolderOpen size={15} strokeWidth={1.8} aria-hidden="true" />
              <span>{storageBusy ? t("settings.storageMoving") : t("settings.storageChange")}</span>
            </button>
          </div>
          <div className="setting-row storage-quota-row">
            <div className="setting-row-info">
              <span>{t("settings.storageQuota")}</span>
            </div>
            <input
              className="storage-quota-input"
              type="text"
              inputMode="numeric"
              pattern="[0-9]*"
              value={storageQuotaDraft}
              onChange={(event) => {
                setStorageQuotaDraft(event.target.value.replace(/\D/g, "").slice(0, 5));
              }}
              onBlur={() => void commitStorageQuota()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  event.currentTarget.blur();
                }
              }}
              aria-label={t("settings.storageQuota")}
            />
            <span className="storage-quota-unit">GiB</span>
          </div>
          {oldStorage && oldStorage.old_root !== oldStorage.new_root && (
            <div className="old-storage-row">
              <span>{t("settings.storageOldData")} ({formatStorageSize(oldStorage.old_size_bytes)})</span>
              <div>
                <button type="button" onClick={() => void deleteOldStorage()}>{t("settings.storageDeleteOld")}</button>
                <button type="button" onClick={() => setOldStorage(null)}>{t("settings.storageKeepOld")}</button>
              </div>
            </div>
          )}
        </section>

        {/* Appearance */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.appearance")}</h3>
            <p>{t("settings.appearanceDescription")}</p>
          </div>

          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.colorMode")}</span>
            </div>
            <div className="theme-cards" role="group" aria-label={t("settings.colorMode")}>
              <button
                type="button"
                className={`theme-card${themePreference === "system" ? " active" : ""}`}
                onClick={() => void handleThemeChange("system")}
                aria-pressed={themePreference === "system"}
              >
                <Monitor className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
                <span>{t("settings.themeSystem")}</span>
              </button>
              <button
                type="button"
                className={`theme-card${themePreference === "light" ? " active" : ""}`}
                onClick={() => void handleThemeChange("light")}
                aria-pressed={themePreference === "light"}
              >
                <Sun className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
                <span>{t("settings.themeLight")}</span>
              </button>
              <button
                type="button"
                className={`theme-card${themePreference === "dark" ? " active" : ""}`}
                onClick={() => void handleThemeChange("dark")}
                aria-pressed={themePreference === "dark"}
              >
                <Moon className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
                <span>{t("settings.themeDark")}</span>
              </button>
            </div>
          </div>

          <div className="setting-row palette-setting-row">
            <div className="setting-row-info">
              <span>{t("settings.colorTheme")}</span>
              <small>{t("settings.colorThemeDescription")}</small>
            </div>
            <div className="theme-cards palette-cards" role="group" aria-label={t("settings.colorTheme")}>
              {COLOR_THEMES.map((option) => (
                <button
                  type="button"
                  key={option}
                  className={`theme-card palette-card${colorTheme === option ? " active" : ""}`}
                  onClick={() => void handleColorThemeChange(option)}
                  aria-pressed={colorTheme === option}
                  title={t(`settings.colorTheme.${option}`)}
                >
                  <div className={`palette-card-preview ${option}`} aria-hidden="true">
                    <span className="palette-preview-rail" />
                    <span className="palette-preview-title">TailSync</span>
                    <span className="palette-preview-row row-one" />
                    <span className="palette-preview-row row-two" />
                  </div>
                  <span className="palette-card-label">{t(`settings.colorTheme.${option}`)}</span>
                  {colorTheme === option && (
                    <Check className="palette-card-check" size={13} strokeWidth={2} aria-hidden="true" />
                  )}
                </button>
              ))}
            </div>
          </div>

          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.language")}</span>
            </div>
            <div className="select-shell">
              <select
                value={settings.language}
                onChange={(e) => {
                  const language = e.target.value as SettingsData["language"];
                  setLocale(language);
                  void update({ language }).then((savedLanguage) => {
                    if (!savedLanguage) {
                      setLocale(settingsRef.current?.language ?? settings.language);
                    }
                  });
                }}
              >
                <option value="zh-CN">简体中文</option>
                <option value="en">English</option>
              </select>
              <ChevronDown size={14} strokeWidth={1.7} aria-hidden="true" />
            </div>
          </div>
        </section>
      </div>

      {pairingOpen && (
        <div className="dialog-backdrop" onMouseDown={() => void closePairing()}>
          <div
            className="pair-dialog"
            ref={pairDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="pair-dialog-title"
            aria-describedby="pair-dialog-status"
            tabIndex={-1}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="confirm-dialog-icon pair-dialog-icon">
              <Settings2 size={22} strokeWidth={1.6} aria-hidden="true" />
            </div>
            <h2 id="pair-dialog-title">
              {t("settings.devicePairing")}
              {(pairingStatus?.peer?.hostname || pairingTarget?.hostname) && ` · ${pairingStatus?.peer?.hostname || pairingTarget?.hostname}`}
            </h2>
            {pairingStatus?.peer ? (
              <>
                <div className="pairing-code" aria-label={t("settings.pairingCode")}>
                  {pairingStatus.peer.verification_code}
                </div>
                <p className="pairing-check-copy" id="pair-dialog-status">
                  {t("settings.compareCode")}
                </p>
                <div className="pairing-peer-fingerprint">{pairingStatus.peer.fingerprint}</div>
                {pairingStatus.phase === "waiting_for_peer" && (
                  <p className="pairing-progress">{t("settings.waitingPeerConfirm")}</p>
                )}
              </>
            ) : (
              <div className="pairing-waiting">
                {pairingBusy || pairingStatus?.phase === "handshaking" ? (
                  <>
                    <span className="pairing-spinner" />
                    <p id="pair-dialog-status">{t("settings.secureHandshake")}</p>
                  </>
                ) : (
                  <div className="pairing-instruction">
                    <span>{t("settings.pairingReady")}</span>
                    <strong id="pair-dialog-status">{t("settings.pairingInstruction")}</strong>
                    <small>
                      {t("settings.pairingExpiresPrefix")} {pairingStatus?.remaining_seconds ?? 0}{" "}
                      {t("settings.pairingExpiresSuffix")}
                    </small>
                  </div>
                )}
              </div>
            )}
            {(pairingError || pairingStatus?.error) && (
              <p className="pair-dialog-error" role="alert">{pairingError || pairingStatus?.error}</p>
            )}
            <div className="confirm-dialog-actions">
              <button type="button" onClick={() => void closePairing()} disabled={pairingBusy}>
                {t("settings.cancel")}
              </button>
              <button
                type="button"
                className="pair-submit"
                onClick={() => void handlePair()}
                disabled={pairingBusy || !pairingStatus?.peer || pairingStatus.peer.local_confirmed}
              >
                {t(pairingStatus?.peer?.local_confirmed
                  ? "settings.confirmed"
                  : "settings.codesMatch")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ── Toast ── */}
      {saved && <div className="toast" role="status">{t("settings.saved")}</div>}
    </div>
  );
}
