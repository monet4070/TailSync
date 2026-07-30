import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  COLOR_THEMES,
  isColorTheme,
  isThemePreference,
  useTheme,
  type ColorTheme,
  type ThemePreference,
} from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import {
  Activity,
  Check,
  ChevronDown,
  Grid2X2,
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

interface SettingsData {
  notifications_enabled: boolean;
  progress_bar_enabled: boolean;
  history_limit: number;
  theme: string;
  color_theme: string;
  language: string;
  enabled_peers: Record<string, boolean>;
  trusted_peer_keys: Record<string, string>;
  trusted_peer_addresses: Record<string, Record<string, string>>;
  paired_peer_endpoints: Record<string, string>;
  connection_mode: "auto" | "lan_only" | "tailscale_only";
}

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

/* ── Component ──────────────────────────────────────────────────── */

export function Settings() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
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
  const { t, locale, setLocale } = useI18n();
  const toastTimer = useRef<number>(0);
  const previousPairingPhase = useRef<PairingStatus["phase"] | null>(null);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((s) => {
        setSettings(s);
        if (isThemePreference(s.theme)) setTheme(s.theme);
        if (isColorTheme(s.color_theme)) setColorTheme(s.color_theme);
        setLocale(s.language);
      })
      .catch(console.error);
  }, [setColorTheme, setLocale, setTheme]);

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
    if (!settings) return false;
    const previous = settings;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      await invoke("update_settings", {
        settingsJson: JSON.stringify(next),
      });
      setSaved(true);
      clearTimeout(toastTimer.current);
      toastTimer.current = setTimeout(() => setSaved(false), 1500);
      return true;
    } catch (e) {
      setSettings(previous);
      console.error("Save settings failed:", e);
      return false;
    }
  };

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
    if (pairingBusy) return;
    setPairingBusy(true);
    try {
      setPairingStatus(await invoke<PairingStatus>("cancel_pairing"));
      setPairingOpen(false);
      setPairingTarget(null);
    } catch (error) {
      setPairingError(String(error));
    } finally {
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
            title="Close"
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
          title="Close"
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
              <h3>{locale === "zh-CN" ? "连接与设备" : "Connections & devices"}</h3>
              <p>
                {locale === "zh-CN"
                  ? "选择发现方式并管理参与同步的设备"
                  : "Choose discovery and manage devices in sync"}
              </p>
            </div>
            <button
              type="button"
              className="icon-button"
              onClick={refreshDevices}
              disabled={devicesLoading}
              title={locale === "zh-CN" ? "刷新设备" : "Refresh devices"}
              aria-label={locale === "zh-CN" ? "刷新设备" : "Refresh devices"}
            >
              <RefreshCw className={devicesLoading ? "spin" : ""} size={16} strokeWidth={1.7} aria-hidden="true" />
            </button>
          </div>

          <div className="connection-mode" role="radiogroup" aria-label={locale === "zh-CN" ? "连接方式" : "Connection mode"}>
            <button
              type="button"
              className={settings.connection_mode === "auto" ? "active" : ""}
              onClick={() => handleConnectionMode("auto")}
              role="radio"
              aria-checked={settings.connection_mode === "auto"}
            >
              {locale === "zh-CN" ? "自动" : "Automatic"}
            </button>
            <button
              type="button"
              className={settings.connection_mode === "lan_only" ? "active" : ""}
              onClick={() => handleConnectionMode("lan_only")}
              role="radio"
              aria-checked={settings.connection_mode === "lan_only"}
            >
              <Wifi size={15} strokeWidth={1.7} aria-hidden="true" />
              {locale === "zh-CN" ? "局域网" : "Local network"}
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
              <strong>{locale === "zh-CN" ? "设备配对" : "Device pairing"}</strong>
              <span>
                {pairingStatus?.pairing_enabled
                  ? `${locale === "zh-CN" ? "等待连接" : "Waiting"} · ${pairingStatus.remaining_seconds}s · ${pairingStatus.failed_attempts}/${pairingStatus.max_failures}`
                  : locale === "zh-CN" ? "当前关闭" : "Currently closed"}
              </span>
            </div>
            <button
              type="button"
              className={pairingStatus?.pairing_enabled ? "pairing-window-close" : "pair-device-action"}
              disabled={pairingBusy}
              onClick={() => pairingStatus?.pairing_enabled ? void closePairing() : void enablePairing()}
            >
              {pairingStatus?.pairing_enabled
                ? locale === "zh-CN" ? "关闭" : "Close"
                : locale === "zh-CN" ? "允许配对" : "Allow pairing"}
            </button>
          </div>

          <div className="device-list" aria-live="polite">
            {devices && (
              <div className="device-row local-device">
                <div className="device-avatar self">{devices.self.hostname.slice(0, 1).toUpperCase()}</div>
                <div className="device-info">
                  <div className="device-name">
                    <span className="device-name-text">{devices.self.hostname}</span>
                    <span>{locale === "zh-CN" ? "本机" : "This device"}</span>
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
                              {locale === "zh-CN" ? "在线" : "Online"}
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
                        {peer.trusted
                          ? locale === "zh-CN" ? "已配对" : "Paired"
                          : locale === "zh-CN" ? "未配对" : "Not paired"}
                      </span>
                    </div>
                    <div className="device-fingerprint">
                      {peer.trusted ? peer.fingerprint : (locale === "zh-CN" ? "等待安全配对" : "Waiting for secure pairing")}
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
                                  ? `${locale === "zh-CN" ? "在线" : "Online"}${route.latency_ms != null ? ` · ${route.latency_ms} ms` : ""}`
                                  : reachabilityStatus === "confirming"
                                    ? locale === "zh-CN" ? "正在确认…" : "Confirming…"
                                    : reachabilityStatus === "discovered"
                                      ? locale === "zh-CN" ? "已发现" : "Discovered"
                                      : locale === "zh-CN" ? "离线" : "Offline"}
                              </span>
                              <span className={`peer-route-connection ${route.connected ? "connected" : "idle"}`}>
                                {route.connected
                                  ? locale === "zh-CN" ? "已连接" : "Connected"
                                  : locale === "zh-CN" ? "未连接" : "Not connected"}
                              </span>
                              <button
                                type="button"
                                className="connection-test-button"
                                disabled={test?.status === "testing"}
                                onClick={() => void testConnection(peer, route)}
                                title={locale === "zh-CN" ? "测试 TailSync TCP 端口" : "Test TailSync TCP port"}
                                aria-label={locale === "zh-CN" ? `测试 ${route.address} 连通性` : `Test ${route.address} connectivity`}
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
                                  {locale === "zh-CN" ? "失败" : "Failed"}
                                </span>
                              )}
                            </div>
                          );
                        })}
                      </div>
                    ) : (
                      <div className="device-address">
                        {locale === "zh-CN" ? "已配对 · 等待设备上线" : "Paired · waiting for device"}
                      </div>
                    )}
                  </div>
                  <div className="device-actions">
                    {peer.trusted ? (
                      <>
                        <label className="toggle" title={peer.enabled ? (locale === "zh-CN" ? "停止同步" : "Disable sync") : (locale === "zh-CN" ? "启用同步" : "Enable sync")}>
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
                          title={locale === "zh-CN" ? "撤销配对" : "Forget paired device"}
                          aria-label={locale === "zh-CN" ? "撤销配对" : "Forget paired device"}
                        >
                          <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
                        </button>
                      </>
                    ) : (
                      <button type="button" className="pair-device-action" onClick={() => void openPairing(peer)}>
                        {locale === "zh-CN" ? "配对" : "Pair"}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}

            {devicesLoading && !devices && (
              <div className="device-list-state">{locale === "zh-CN" ? "正在发现设备..." : "Discovering devices..."}</div>
            )}
            {!devicesLoading && devices && devices.peers.length === 0 && (
              <div className="device-list-state">
                {locale === "zh-CN" ? "暂未发现其他在线设备" : "No other online devices found"}
              </div>
            )}
            {!devicesLoading && devicesError && (
              <div className="device-list-state error">
                {settings.connection_mode === "tailscale_only"
                  ? locale === "zh-CN" ? "无法读取 Tailscale，请确认其已安装并登录" : "Tailscale is unavailable. Check that it is installed and signed in."
                  : locale === "zh-CN" ? "局域网发现失败，请检查防火墙和网络权限" : "LAN discovery failed. Check firewall and network access."}
              </div>
            )}
          </div>
        </section>

        {/* General */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.general")}</h3>
            <p>
              {locale === "zh-CN"
                ? "控制 TailSync 的行为和通知方式"
                : "Control TailSync behaviour and notifications"}
            </p>
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
              <small>
                {locale === "zh-CN"
                  ? "收到新内容时弹出系统通知"
                  : "Show system notifications for new content"}
              </small>
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
              <small>
                {locale === "zh-CN"
                  ? "在底部显示文件传输进度"
                  : "Show file transfer progress at the bottom"}
              </small>
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
            <p>
              {locale === "zh-CN"
                ? "管理剪贴板历史存储"
                : "Manage clipboard history storage"}
            </p>
          </div>

          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.historyLimit")}</span>
              <small>
                {locale === "zh-CN"
                  ? `最多保留 ${settings.history_limit} 条记录`
                  : `Keep up to ${settings.history_limit} entries`}
              </small>
            </div>
            <input
              type="range"
              min={10}
              max={500}
              value={settings.history_limit}
              onChange={(e) =>
                update({ history_limit: parseInt(e.target.value) })
              }
            />
            <span className="range-value">{settings.history_limit}</span>
          </div>
        </section>

        {/* Appearance */}
        <section className="setting-group">
          <div className="setting-group-header">
            <h3>{t("settings.appearance")}</h3>
            <p>
              {locale === "zh-CN"
                ? "自定义界面主题和语言"
                : "Customise the look and language"}
            </p>
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
                  update({ language: e.target.value });
                  setLocale(e.target.value);
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
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="confirm-dialog-icon pair-dialog-icon">
              <Settings2 size={22} strokeWidth={1.6} aria-hidden="true" />
            </div>
            <h2>
              {locale === "zh-CN" ? "设备配对" : "Device pairing"}
              {(pairingStatus?.peer?.hostname || pairingTarget?.hostname) && ` · ${pairingStatus?.peer?.hostname || pairingTarget?.hostname}`}
            </h2>
            {pairingStatus?.peer ? (
              <>
                <div className="pairing-code" aria-label={locale === "zh-CN" ? "配对验证码" : "Pairing verification code"}>
                  {pairingStatus.peer.verification_code}
                </div>
                <p className="pairing-check-copy">
                  {locale === "zh-CN" ? "请确认另一台设备显示相同验证码" : "Confirm that the other device shows the same code"}
                </p>
                <div className="pairing-peer-fingerprint">{pairingStatus.peer.fingerprint}</div>
                {pairingStatus.phase === "waiting_for_peer" && (
                  <p className="pairing-progress">{locale === "zh-CN" ? "已确认，等待对端确认..." : "Confirmed, waiting for the other device..."}</p>
                )}
              </>
            ) : (
              <div className="pairing-waiting">
                {pairingBusy || pairingStatus?.phase === "handshaking" ? (
                  <>
                    <span className="pairing-spinner" />
                    <p>{locale === "zh-CN" ? "正在建立安全连接..." : "Establishing a secure connection..."}</p>
                  </>
                ) : (
                  <div className="pairing-instruction">
                    <span>{locale === "zh-CN" ? "配对已开启" : "Pairing is ready"}</span>
                    <strong>
                      {locale === "zh-CN"
                        ? "请在另一端设备列表点击配对按钮"
                        : "On the other device, click Pair in the device list"}
                    </strong>
                    <small>
                      {locale === "zh-CN"
                        ? `窗口将在 ${pairingStatus?.remaining_seconds ?? 0} 秒后关闭`
                        : `This window closes in ${pairingStatus?.remaining_seconds ?? 0} seconds`}
                    </small>
                  </div>
                )}
              </div>
            )}
            {(pairingError || pairingStatus?.error) && <p className="pair-dialog-error">{pairingError || pairingStatus?.error}</p>}
            <div className="confirm-dialog-actions">
              <button type="button" onClick={() => void closePairing()} disabled={pairingBusy}>
                {locale === "zh-CN" ? "取消" : "Cancel"}
              </button>
              <button
                type="button"
                className="pair-submit"
                onClick={() => void handlePair()}
                disabled={pairingBusy || !pairingStatus?.peer || pairingStatus.peer.local_confirmed}
              >
                {pairingStatus?.peer?.local_confirmed
                  ? locale === "zh-CN" ? "已确认" : "Confirmed"
                  : locale === "zh-CN" ? "验证码一致" : "Codes match"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ── Toast ── */}
      {saved && <div className="toast">{t("settings.saved")}</div>}
    </div>
  );
}
