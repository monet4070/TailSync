import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";

/* ── Types ──────────────────────────────────────────────────────── */

interface SettingsData {
  notifications_enabled: boolean;
  progress_bar_enabled: boolean;
  history_limit: number;
  theme: string;
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
  current_address?: string;
  status?: "discovered" | "confirming" | "online" | "connected" | "offline";
}

interface PeersResponse {
  self: {
    hostname: string;
    tailscale_ip: string;
    connection_mode: "auto" | "lan_only" | "tailscale_only";
    public_key: string;
    fingerprint: string;
  };
  peers: PeerDevice[];
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
  const { theme, setTheme, themePreference } = useTheme();
  const { t, locale, setLocale } = useI18n();
  const toastTimer = useRef<number>(0);
  const previousPairingPhase = useRef<PairingStatus["phase"] | null>(null);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((s) => {
        setSettings(s);
        setLocale(s.language);
      })
      .catch(console.error);
  }, [setLocale]);

  const connectionMode = settings?.connection_mode;
  useEffect(() => {
    if (!connectionMode) return;
    let active = true;
    const load = async () => {
      setDevicesLoading(true);
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
        if (active) setDevicesLoading(false);
      }
    };
    load();
    const timer = window.setInterval(load, 5000);
    let unlisten: (() => void) | undefined;
    void listen("peer-health-changed", () => void load()).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
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
      await invoke("refresh_peers");
      const result = await invoke<PeersResponse>("get_peers");
      setDevices(result);
      setDevicesError(result.discovery_error ?? "");
    } catch (error) {
      setDevices(null);
      setDevicesError(String(error));
    } finally {
      setDevicesLoading(false);
    }
  };

  const peerStatus = (peer: PeerDevice) => {
    const status = peer.status ?? (peer.current_address ? "connected" : peer.online ? "online" : "offline");
    const labels = locale === "zh-CN"
      ? { connected: "已连接", online: "在线", confirming: "正在确认…", discovered: "已发现", offline: "离线" }
      : { connected: "Connected", online: "Online", confirming: "Confirming…", discovered: "Discovered", offline: "Offline" };
    return { status, label: labels[status] };
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

  const copyIdentityKey = async () => {
    if (!devices?.self.public_key) return;
    try {
      await navigator.clipboard.writeText(devices.self.public_key);
      setSaved(true);
      clearTimeout(toastTimer.current);
      toastTimer.current = setTimeout(() => setSaved(false), 1500);
    } catch (error) {
      console.error("Copy identity key failed:", error);
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

  const handleThemeChange = (value: string) => {
    update({ theme: value });
    setTheme(value as "light" | "dark" | "system");
  };

  if (!settings) {
    return (
      <div className={`app ${theme}`}>
        {/* Title bar */}
        <div className="titlebar" data-tauri-drag-region>
          <div className="titlebar-brand">
            <div className="titlebar-logo">
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 15l-4-4 1.41-1.41L11 14.17l6.59-6.59L19 9l-8 8z" />
              </svg>
            </div>
            <span className="titlebar-text">{t("settings.title")}</span>
            <span className="titlebar-badge">v2</span>
          </div>
          <button
            className="titlebar-close"
            onClick={() => getCurrentWindow().hide()}
            title="Close"
          >
            ✕
          </button>
        </div>
        <div className="loading-text">{t("settings.loading")}</div>
      </div>
    );
  }

  return (
    <div className={`app ${theme}`}>
      {/* ── Title bar ── */}
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <div className="titlebar-logo">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 15l-4-4 1.41-1.41L11 14.17l6.59-6.59L19 9l-8 8z" />
            </svg>
          </div>
          <span className="titlebar-text">{t("settings.title")}</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => getCurrentWindow().hide()}
          title="Close"
        >
          ✕
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
              <svg className={devicesLoading ? "spin" : ""} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <path d="M20 11a8 8 0 1 0-2.34 5.66M20 4v7h-7" />
              </svg>
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
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <path d="M5 12.55a11 11 0 0 1 14.08 0M8.53 16.11a6 6 0 0 1 6.95 0M12 20h.01" />
              </svg>
              {locale === "zh-CN" ? "局域网" : "Local network"}
            </button>
            <button
              type="button"
              className={settings.connection_mode === "tailscale_only" ? "active" : ""}
              onClick={() => handleConnectionMode("tailscale_only")}
              role="radio"
              aria-checked={settings.connection_mode === "tailscale_only"}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <circle cx="7" cy="7" r="2" /><circle cx="17" cy="7" r="2" />
                <circle cx="7" cy="17" r="2" /><circle cx="17" cy="17" r="2" />
                <path d="M9 7h6M7 9v6M17 9v6M9 17h6" />
              </svg>
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
                    {devices.self.hostname}
                    <span>{locale === "zh-CN" ? "本机" : "This device"}</span>
                  </div>
                  <div className="device-address">{devices.self.tailscale_ip}</div>
                  <div className="device-fingerprint">{devices.self.fingerprint}</div>
                </div>
                <button
                  type="button"
                  className="icon-button"
                  onClick={copyIdentityKey}
                  title={locale === "zh-CN" ? "复制本机公钥" : "Copy device public key"}
                  aria-label={locale === "zh-CN" ? "复制本机公钥" : "Copy device public key"}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                    <rect x="8" y="8" width="12" height="12" rx="2" />
                    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
                  </svg>
                </button>
                <span className="device-status online">{locale === "zh-CN" ? "在线" : "Online"}</span>
              </div>
            )}

            {devices?.peers.map((peer) => {
              const health = peerStatus(peer);
              return (
              <div className="device-row" key={`${peer.hostname}-${peer.address}`}>
                <div className="device-avatar">{peer.hostname.slice(0, 1).toUpperCase()}</div>
                <div className="device-info">
                  <div className="device-name">{peer.hostname}</div>
                  <div className="device-address">
                    {peer.address || peer.tailscale_ip || (locale === "zh-CN" ? "已配对 · 等待设备上线" : "Paired · waiting for device")}
                    {peer.current_interface && ` · ${peer.current_interface === "lan" ? "LAN" : "Tailscale"}`}
                  </div>
                  <div className="device-fingerprint">
                    {peer.trusted
                      ? peer.fingerprint
                      : locale === "zh-CN" ? "未配对" : "Not paired"}
                  </div>
                </div>
                <span className={`device-status ${health.status}`}>{health.label}</span>
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
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                        <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
                      </svg>
                    </button>
                  </>
                ) : (
                  <button type="button" className="pair-device-action" onClick={() => void openPairing(peer)}>
                    {locale === "zh-CN" ? "配对" : "Pair"}
                  </button>
                )}
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
              <span>{t("settings.theme")}</span>
            </div>
            <div className="theme-cards">
              <div
                className={`theme-card${themePreference === "system" ? " active" : ""}`}
                onClick={() => handleThemeChange("system")}
              >
                <div className="theme-card-preview system">
                  <div className="half" />
                  <div className="half" />
                </div>
                <span>{t("settings.themeSystem")}</span>
              </div>
              <div
                className={`theme-card${themePreference === "light" ? " active" : ""}`}
                onClick={() => handleThemeChange("light")}
              >
                <div className="theme-card-preview light" />
                <span>{t("settings.themeLight")}</span>
              </div>
              <div
                className={`theme-card${themePreference === "dark" ? " active" : ""}`}
                onClick={() => handleThemeChange("dark")}
              >
                <div className="theme-card-preview dark" />
                <span>{t("settings.themeDark")}</span>
              </div>
            </div>
          </div>

          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.language")}</span>
            </div>
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
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-1.42 1.42-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V20h-2v-.08a1.7 1.7 0 0 0-1.03-1.56 1.7 1.7 0 0 0-1.88.34l-.06.06-1.42-1.42.06-.06A1.7 1.7 0 0 0 9.4 15a1.7 1.7 0 0 0-1.56-1.03H7v-2h.84A1.7 1.7 0 0 0 9.4 11a1.7 1.7 0 0 0-.34-1.88L9 9.06l1.42-1.42.06.06a1.7 1.7 0 0 0 1.88.34A1.7 1.7 0 0 0 13.4 6.48V6h2v.48a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.88-.34l.06-.06 1.42 1.42-.06.06a1.7 1.7 0 0 0-.34 1.88 1.7 1.7 0 0 0 1.56 1.03H21v2h-.48A1.7 1.7 0 0 0 19.4 15Z" />
              </svg>
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
                <span className="pairing-spinner" />
                <p>
                  {pairingBusy || pairingStatus?.phase === "handshaking"
                    ? locale === "zh-CN" ? "正在建立安全连接..." : "Establishing a secure connection..."
                    : locale === "zh-CN" ? "等待另一台设备连接..." : "Waiting for another device..."}
                </p>
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
