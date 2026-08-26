import { useCallback, useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme, type ThemePreference } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { LatestRequest, SerialTaskQueue } from "../utils/asyncControl";
import type { SettingsData } from "../types/settings.generated";
import {
  changeStorageLocation,
  closeSettingsWindow,
  deleteOldStorage,
  deleteThemeV2,
  formatThemeError,
  forgetPeer,
  getSettings,
  getStorageStatus,
  setHistoryShortcut,
  installThemeV2,
  listThemesV2,
  getLocalThemeSettingsV2,
  setLocalThemeSettingsV2,
  rollbackThemeV2,
  setSyncEnabled,
  setSyncShortcut,
  updateSettings,
  type PeerDevice,
  type PeerRoute,
  type StorageMigrationResult,
  type StorageStatus,
  type ThemeV2Descriptor,
} from "../tailsyncClient";
import { ThemePackagePreview } from "../components/ThemePackagePreview";
import {
  applyThemePackageOperation,
  validateThemePackageForPreview,
  type PendingThemePackage,
  type ThemePackageOperation,
} from "../utils/themePackageWorkflow";
import { useConnectionTests } from "../hooks/useConnectionTests";
import { useDevices } from "../hooks/useDevices";
import { usePairing } from "../hooks/usePairing";
import {
  DEFAULT_HISTORY_SHORTCUT,
  DEFAULT_SYNC_SHORTCUT,
  useShortcutRecorder,
} from "../hooks/useShortcutRecorder";
import { useUpdater } from "../hooks/useUpdater";
import {
  Activity,
  Check,
  ChevronDown,
  FolderOpen,
  Grid2X2,
  HardDrive,
  Keyboard,
  Monitor,
  Moon,
  Pencil,
  RefreshCw,
  RotateCcw,
  Settings2,
  Sun,
  Trash2,
  Upload,
  Wifi,
  X,
} from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";
import { shortcutKeycaps } from "../utils/shortcut";
import { pairingAddressForPeer } from "../utils/pairingAddress";
import { routeSupportsLatencyTest } from "../utils/peerRoute";

/* ── Types ──────────────────────────────────────────────────────── */

const routeInterfaceLabel = (routeInterface: PeerRoute["interface"]) => {
  if (routeInterface === "lan") return "LAN";
  if (routeInterface === "iroh") return "Iroh";
  return "Tailscale";
};

const peerCanSync = (peer: PeerDevice) =>
  peer.trusted && peer.enabled && (
    Boolean(peer.current_interface)
    || peer.online
    || Boolean(peer.address)
    || Boolean(peer.tailscale_ip)
    || Boolean(peer.routes?.some((route) => Boolean(route.address)))
  );

const GIB = 1024 * 1024 * 1024;

function formatStorageSize(bytes: number) {
  return `${(bytes / GIB).toFixed(bytes >= GIB ? 1 : 2)} GiB`;
}

type ShortcutRecorder = ReturnType<typeof useShortcutRecorder>;

function ShortcutSettingRow({
  recorder,
  currentShortcut,
  defaultShortcut,
  title,
  description,
  recordLabel,
  t,
  disabled = false,
}: {
  recorder: ShortcutRecorder;
  currentShortcut: string;
  defaultShortcut: string;
  title: string;
  description: string;
  recordLabel: string;
  t: (key: string) => string;
  disabled?: boolean;
}) {
  return (
    <div className="setting-row shortcut-row">
      <div className="setting-row-info">
        <span>{title}</span>
        <small>{description}</small>
      </div>
      <div className="shortcut-control">
        <button
          ref={recorder.shortcutTriggerRef}
          type="button"
          className="shortcut-recorder"
          disabled={disabled || recorder.shortcutBusy}
          onClick={() => void recorder.startShortcutRecording()}
          aria-haspopup="dialog"
          aria-expanded={recorder.shortcutRecording}
          aria-label={recordLabel}
        >
          <Keyboard size={16} strokeWidth={1.7} aria-hidden="true" />
          {shortcutKeycaps(recorder.shortcutDraft).length > 0 ? (
            <span className="shortcut-keycaps" aria-label={recorder.shortcutDraft}>
              {shortcutKeycaps(recorder.shortcutDraft).map((key, index) => (
                <kbd key={`${key}-${index}`}>{key}</kbd>
              ))}
            </span>
          ) : (
            <span className="shortcut-empty">{t("settings.shortcutDisabled")}</span>
          )}
          <Pencil className="shortcut-edit-icon" size={13} strokeWidth={1.8} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="shortcut-icon-button"
          disabled={disabled || recorder.shortcutBusy || recorder.shortcutRecording || recorder.shortcutDraft === defaultShortcut}
          onClick={() => {
            recorder.setShortcutDraft(defaultShortcut);
            void recorder.commitShortcut(defaultShortcut);
          }}
          title={t("settings.shortcutReset")}
          aria-label={t("settings.shortcutReset")}
        >
          <RotateCcw size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="shortcut-icon-button"
          disabled={disabled || recorder.shortcutBusy || recorder.shortcutRecording || !currentShortcut}
          onClick={() => {
            recorder.setShortcutDraft("");
            void recorder.commitShortcut("");
          }}
          title={t("settings.shortcutClear")}
          aria-label={t("settings.shortcutClear")}
        >
          <X size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}

function ShortcutRecorderDialog({
  recorder,
  title,
  prompt,
  t,
}: {
  recorder: ShortcutRecorder;
  title: string;
  prompt: string;
  t: (key: string) => string;
}) {
  if (!recorder.shortcutRecording) return null;
  const titleId = `shortcut-dialog-title-${title.replace(/\s+/g, "-")}`;
  return (
    <div className="dialog-backdrop" onMouseDown={() => void recorder.cancelShortcutRecording()}>
      <div
        className="shortcut-dialog"
        ref={recorder.shortcutDialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="shortcut-dialog-header">
          <div className="shortcut-dialog-icon">
            <Keyboard size={20} strokeWidth={1.7} aria-hidden="true" />
          </div>
          <div>
            <h2 id={titleId}>{title}</h2>
            <p>{prompt}</p>
          </div>
        </div>
        <button
          ref={recorder.shortcutCaptureRef}
          type="button"
          className={`shortcut-capture-target${recorder.shortcutCaptureActive ? " active" : " captured"}`}
          onClick={recorder.restartShortcutCapture}
          onKeyDown={recorder.handleShortcutCaptureEvent}
          disabled={recorder.shortcutBusy}
          aria-label={recorder.shortcutCaptureActive
            ? t("settings.shortcutRecording")
            : t("settings.shortcutRecordAgain")}
        >
          {recorder.shortcutPreviewKeys.length > 0 ? (
            <span className="shortcut-keycaps shortcut-dialog-keycaps">
              {recorder.shortcutPreviewKeys.map((key, index) => (
                <kbd key={`${key}-${index}`}>{key}</kbd>
              ))}
            </span>
          ) : (
            <span className="shortcut-capture-placeholder">{t("settings.shortcutRecording")}</span>
          )}
          {!recorder.shortcutCaptureActive && (
            <span className="shortcut-capture-again">{t("settings.shortcutRecordAgain")}</span>
          )}
        </button>
        <div className="shortcut-dialog-message" aria-live="polite">
          {recorder.shortcutDialogError && (
            <span className="error" role="alert">{recorder.shortcutDialogError}</span>
          )}
          {!recorder.shortcutDialogError && recorder.shortcutCandidate && (
            <span className="ready">{t("settings.shortcutCaptured")}</span>
          )}
        </div>
        <div className="shortcut-dialog-actions">
          <button
            type="button"
            onClick={() => void recorder.cancelShortcutRecording()}
            disabled={recorder.shortcutBusy}
          >
            {t("settings.shortcutCancel")}
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => void recorder.confirmShortcut()}
            disabled={!recorder.shortcutCandidate || recorder.shortcutBusy}
          >
            {t("settings.shortcutSave")}
          </button>
        </div>
      </div>
    </div>
  );
}

/* ── Component ──────────────────────────────────────────────────── */

export function Settings() {
  const [v2Themes, setV2Themes] = useState<ThemeV2Descriptor[]>([]);
  const [v2Active, setV2Active] = useState("builtin:canvas@1");
  const [v2HighContrast, setV2HighContrast] = useState(false);
  const [pendingThemeImport, setPendingThemeImport] = useState<PendingThemePackage | null>(null);
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [historyLimitDraft, setHistoryLimitDraft] = useState(100);
  const [storageQuotaDraft, setStorageQuotaDraft] = useState("10");
  const [storageStatus, setStorageStatus] = useState<StorageStatus | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [oldStorage, setOldStorage] = useState<StorageMigrationResult | null>(null);
  const [saved, setSaved] = useState(false);
  const [errorMessage, setErrorMessage] = useState("");
  const {
    theme,
    setTheme,
    themePreference,
  } = useTheme();
  const { t, setLocale, locale } = useI18n();
  const toastTimer = useRef<number>(0);
  const settingsRef = useRef<SettingsData | null>(null);
  const saveQueue = useRef(new SerialTaskQueue());
  const settingsUpdates = useRef(new LatestRequest());
  const currentShortcut = useCallback(
    () => settingsRef.current?.sync_shortcut ?? null,
    [],
  );
  const refreshV2Themes = useCallback(async () => {
    try {
      const [themes, local] = await Promise.all([listThemesV2(), getLocalThemeSettingsV2()]);
      setV2Themes(themes); setV2Active(local.activeThemeId); setV2HighContrast(local.highContrast);
    } catch {
      setV2Themes([]);
    }
  }, []);
  useEffect(() => { void refreshV2Themes(); }, [refreshV2Themes]);
  const currentHistoryShortcut = useCallback(
    () => settingsRef.current?.history_shortcut ?? null,
    [],
  );
  const applyShortcut = useCallback((shortcut: string) => {
    const current = settingsRef.current;
    if (!current) return;
    settingsRef.current = { ...current, sync_shortcut: shortcut };
    setSettings(settingsRef.current);
  }, []);
  const applyHistoryShortcut = useCallback((shortcut: string) => {
    const current = settingsRef.current;
    if (!current) return;
    settingsRef.current = { ...current, history_shortcut: shortcut };
    setSettings(settingsRef.current);
  }, []);
  const showSavedToast = useCallback(() => {
    setSaved(true);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setSaved(false), 1500);
  }, []);
  const showError = useCallback((message: string) => setErrorMessage(message), []);
  const syncShortcutRecorder = useShortcutRecorder({
    defaultShortcut: DEFAULT_SYNC_SHORTCUT,
    currentShortcut,
    setShortcut: setSyncShortcut,
    applyShortcut,
    showSavedToast,
    showError,
  });
  const historyShortcutRecorder = useShortcutRecorder({
    defaultShortcut: DEFAULT_HISTORY_SHORTCUT,
    currentShortcut: currentHistoryShortcut,
    setShortcut: setHistoryShortcut,
    applyShortcut: applyHistoryShortcut,
    showSavedToast,
    showError,
  });
  const setSyncShortcutDraft = syncShortcutRecorder.setShortcutDraft;
  const setHistoryShortcutDraft = historyShortcutRecorder.setShortcutDraft;
  const applyPeerEnabled = useCallback((hostname: string, enabled: boolean) => {
    setSettings((current) => current ? {
      ...current,
      enabled_peers: { ...current.enabled_peers, [hostname]: enabled },
    } : current);
  }, []);
  const {
    devices,
    devicesLoading,
    devicesError,
    refreshDevices,
    onDevicesRefreshed,
    resetDevices,
    handlePeerToggle,
  } = useDevices({
    connectionMode: settings?.connection_mode,
    applyPeerEnabled,
  });
  const { connectionTests, handleTestConnection } = useConnectionTests(onDevicesRefreshed);
  const {
    updateStatus,
    availableUpdate,
    updatePhase,
    updateMessage,
    updateBusy,
    handleCheckForUpdate,
    handleInstallUpdate,
  } = useUpdater();

  useEffect(() => {
    getSettings()
      .then((s) => {
        settingsRef.current = s;
        setSettings(s);
        setHistoryLimitDraft(s.history_limit);
        setStorageQuotaDraft(String(Math.round(s.storage_quota_bytes / GIB)));
        setSyncShortcutDraft(s.sync_shortcut);
        setHistoryShortcutDraft(s.history_shortcut);
        // Theme selection is intentionally not hydrated from synchronised
        // AppSettings. useTheme reads Core local-settings.json instead.
        setLocale(s.language);
      })
      .catch(console.error);
    getStorageStatus().then(setStorageStatus).catch(console.error);
  }, [
    setLocale,
    setSyncShortcutDraft,
    setHistoryShortcutDraft,
  ]);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<{ enabled: boolean }>("sync-state-changed", ({ payload }) => {
      if (!active) return;
      const current = settingsRef.current;
      if (!current || current.sync_enabled === payload.enabled) return;
      const next = { ...current, sync_enabled: payload.enabled };
      settingsRef.current = next;
      setSettings(next);
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  const update = async (patch: Partial<SettingsData>) => {
    const previous = settingsRef.current;
    if (!previous) return false;
    const next = { ...previous, ...patch };
    setErrorMessage("");
    settingsRef.current = next;
    setSettings(next);
    const generation = settingsUpdates.current.begin();
    const save = saveQueue.current.enqueue(() =>
      updateSettings(next),
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
          const canonical = await getSettings();
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
      setErrorMessage(t("settings.saveFailed"));
      return false;
    }
  };

  const commitHistoryLimit = async () => {
    if (historyLimitDraft === settingsRef.current?.history_limit) return;
    if (!(await update({ history_limit: historyLimitDraft }))) {
      setHistoryLimitDraft(settingsRef.current?.history_limit ?? historyLimitDraft);
    }
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
      setErrorMessage("");
      const result = await changeStorageLocation(parent);
      setOldStorage(result);
      const [canonical, status] = await Promise.all([
        getSettings(),
        getStorageStatus(),
      ]);
      settingsRef.current = canonical;
      setSettings(canonical);
      setStorageQuotaDraft(String(Math.round(canonical.storage_quota_bytes / GIB)));
      setStorageStatus(status);
    } catch (error) {
      console.error("Storage migration failed:", error);
      setErrorMessage(t("settings.storageActionFailed"));
    } finally {
      setStorageBusy(false);
    }
  };

  const handleDeleteOldStorage = async () => {
    if (!oldStorage) return;
    try {
      setErrorMessage("");
      await deleteOldStorage(oldStorage.old_root);
      setOldStorage(null);
    } catch (error) {
      console.error("Delete old storage failed:", error);
      setErrorMessage(t("settings.storageActionFailed"));
    }
  };

  const {
    pairingTarget,
    pairingStatus,
    pairingOpen,
    pairingError,
    pairingBusy,
    pairDialogRef,
    handleEnablePairing,
    openPairing,
    closePairing,
    handlePair,
  } = usePairing({ refreshDevices });

  const handleConnectionMode = async (mode: "auto" | "lan_only" | "tailscale_only") => {
    if (mode === settings?.connection_mode) return;
    resetDevices();
    if (await update({ connection_mode: mode })) {
      await refreshDevices();
    }
  };

  const handleForget = async (peer: PeerDevice) => {
    try {
      await forgetPeer(peer.hostname);
      await refreshDevices();
    } catch (error) {
      console.error("Forget peer failed:", error);
    }
  };

  const handleThemeChange = async (value: ThemePreference) => {
    if (value === themePreference) return;
    setTheme(value);
  };

  const chooseThemePackage = async (operation: ThemePackageOperation) => {
    const path = await open({
      multiple: false,
      directory: false,
      title: t(operation.kind === "install" ? "settings.customThemeImportTitle" : "settings.customThemeUpdateTitle"),
      filters: [{ name: "TailSync V2 theme", extensions: ["tailsync-theme"] }],
    });
    if (typeof path !== "string") return;
    try {
      setErrorMessage("");
      setPendingThemeImport(await validateThemePackageForPreview(path, operation));
    } catch (error) {
      setErrorMessage(formatThemeError(error));
    }
  };

  const handleImportTheme = () => chooseThemePackage({ kind: "install" });

  const handleUpdateTheme = (entry: ThemeV2Descriptor) => (
    chooseThemePackage({ kind: "update", themeId: entry.id, installedVersion: entry.version })
  );

  const confirmThemeImport = async () => {
    if (!pendingThemeImport) return;
    if (pendingThemeImport.operation.kind === "update" && pendingThemeImport.versionRelation !== "upgrade") {
      const key = pendingThemeImport.versionRelation === "same"
        ? "settings.customThemeReplaceConfirm"
        : "settings.customThemeDowngradeConfirm";
      const versions = `${pendingThemeImport.operation.installedVersion} → ${pendingThemeImport.candidateVersion}`;
      if (!window.confirm(`${t(key)}\n${versions}`)) return;
    }
    try {
      await applyThemePackageOperation(pendingThemeImport, {
        install: installThemeV2,
        refresh: refreshV2Themes,
      });
      setPendingThemeImport(null);
      showSavedToast();
    } catch (error) {
      setErrorMessage(formatThemeError(error));
    }
  };

  const selectV2Theme = async (id: string) => {
    const previous = v2Active;
    setV2Active(id);
    try { await setLocalThemeSettingsV2({ activeThemeId: id, appearance: themePreference, highContrast: v2HighContrast }); }
    catch (error) { setV2Active(previous); setErrorMessage(formatThemeError(error)); }
  };

  const deleteV2Theme = async (entry: ThemeV2Descriptor) => {
    if (entry.source !== "custom" && entry.status === "valid") return;
    if (!window.confirm(`${t("settings.customThemeDelete")} "${entry.name[locale] ?? entry.name.en ?? entry.id}"?`)) return;
    try {
      await deleteThemeV2(entry.id, entry.storageHandle);
      await refreshV2Themes();
    } catch (error) { setErrorMessage(formatThemeError(error)); }
  };

  const rollbackV2Theme = async (entry: ThemeV2Descriptor) => {
    if (entry.source !== "custom") return;
    if (!window.confirm(`${t("settings.customThemeRollbackConfirm")} "${entry.name[locale] ?? entry.name.en ?? entry.id}"?`)) return;
    try {
      await rollbackThemeV2(entry.id);
      await refreshV2Themes();
      showSavedToast();
    } catch (error) {
      setErrorMessage(formatThemeError(error));
    }
  };

  const setGlobalSync = async (enabled: boolean) => {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, sync_enabled: enabled };
    settingsRef.current = next;
    setSettings(next);
    try {
      await setSyncEnabled(enabled);
    } catch (error) {
      console.error("Could not change sync state:", error);
      settingsRef.current = current;
      setSettings(current);
      setErrorMessage(t("settings.saveFailed"));
    }
  };

  const appClassName = `app settings-window ${theme}`;

  if (!settings) {
    return (
      <div className={appClassName}>
        {/* Title bar */}
        <div className="titlebar" data-tauri-drag-region>
          <div className="titlebar-brand">
            <ThemeLogo />
            <span className="titlebar-text">{t("settings.title")}</span>
            <span className="titlebar-badge">v2</span>
          </div>
          <button
            className="titlebar-close"
            onClick={() => void closeSettingsWindow()}
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
          <ThemeLogo />
          <span className="titlebar-text">{t("settings.title")}</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => void closeSettingsWindow()}
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
              onClick={() => pairingStatus?.pairing_enabled ? void closePairing() : void handleEnablePairing()}
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
                  {devices.self.iroh_endpoint_id && (
                    <div className="device-fingerprint" title={devices.self.iroh_endpoint_id}>
                      iroh: {devices.self.iroh_endpoint_id}
                    </div>
                  )}
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
                              {routeInterfaceLabel(route.interface)}
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
                    rtt_capable: peer.current_interface !== "iroh",
                  } satisfies PeerRoute]
                  : [];
              const pairingAddress = pairingAddressForPeer(peer);
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
                    <div className={`peer-sync-state ${peerCanSync(peer) ? "ready" : "blocked"}`}>
                      {peerCanSync(peer)
                        ? t("settings.syncReady")
                        : !peer.trusted
                          ? t("settings.syncNeedsPairing")
                          : !peer.enabled
                            ? t("settings.syncPeerPaused")
                            : t("settings.syncNoRoute")}
                    </div>
                    {peer.required_protocol_version != null && (
                      <div className="peer-protocol-warning" role="status">
                        {t("settings.protocolUpgradeRequired").replace(
                          "{version}",
                          String(peer.required_protocol_version),
                        )}
                      </div>
                    )}
                    {routes.length > 0 ? (
                      <div className="peer-route-list">
                        {routes.map((route) => {
                          const testKey = `${peer.hostname}|${route.interface}|${route.address}`;
                          const test = connectionTests[testKey];
                          const reachabilityStatus = route.status === "connected" ? "online" : route.status;
                          return (
                            <div className="peer-route" key={`${route.interface}-${route.address}`}>
                              <span className="peer-route-address" title={route.address}>{route.address}</span>
                              <span className={`peer-route-interface ${route.interface}`}>
                                {routeInterfaceLabel(route.interface)}
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
                                disabled={test?.status === "testing"
                                  || !routeSupportsLatencyTest(route)}
                                onClick={() => void handleTestConnection(peer, route)}
                                title={!routeSupportsLatencyTest(route)
                                  ? t("settings.testRouteRediscover")
                                  : t(route.interface === "iroh" ? "settings.testRoute" : "settings.testTcpPort")}
                                aria-label={`${t("settings.testAddress")}: ${route.address}`}
                              >
                                {test?.status === "testing"
                                  ? <RefreshCw className="spin" size={16} strokeWidth={1.7} aria-hidden="true" />
                                  : <Activity size={16} strokeWidth={1.7} aria-hidden="true" />}
                              </button>
                              {test?.status === "success" && (
                                <span className="connection-test-result success">
                                  {test.latency_ms} ms
                                  {test.path === "relay" && ` · ${t("settings.relayPath")}`}
                                </span>
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
                      <button
                        type="button"
                        className="pair-device-action"
                        onClick={() => void openPairing(peer)}
                        disabled={!pairingAddress}
                        title={pairingAddress ? undefined : t("settings.pairUnavailable")}
                      >
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
            className="setting-row setting-row--toggle"
            onClick={() => void setGlobalSync(!settings.sync_enabled)}
          >
            <div className="setting-row-info">
              <span>{t("settings.syncEnabled")}</span>
              <small>{t("settings.syncEnabledDescription")}</small>
            </div>
            <label className="toggle" onClick={(event) => event.stopPropagation()}>
              <input
                type="checkbox"
                checked={settings.sync_enabled}
                onChange={(event) => void setGlobalSync(event.target.checked)}
              />
              <div className="toggle-track" />
            </label>
          </div>

          <ShortcutSettingRow
            recorder={syncShortcutRecorder}
            currentShortcut={settings.sync_shortcut}
            defaultShortcut={DEFAULT_SYNC_SHORTCUT}
            title={t("settings.syncShortcut")}
            description={t("settings.syncShortcutDescription")}
            recordLabel={t("settings.shortcutRecord")}
            t={t}
            disabled={historyShortcutRecorder.shortcutRecording}
          />

          <ShortcutSettingRow
            recorder={historyShortcutRecorder}
            currentShortcut={settings.history_shortcut}
            defaultShortcut={DEFAULT_HISTORY_SHORTCUT}
            title={t("settings.historyShortcut")}
            description={t("settings.historyShortcutDescription")}
            recordLabel={t("settings.historyShortcutRecord")}
            t={t}
            disabled={syncShortcutRecorder.shortcutRecording}
          />

          <div
            className="setting-row setting-row--toggle"
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
            className="setting-row setting-row--toggle"
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
                <button type="button" onClick={() => void handleDeleteOldStorage()}>{t("settings.storageDeleteOld")}</button>
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
              {v2Themes.map((entry) => {
                const active = entry.id === v2Active;
                const label = entry.name[locale] ?? entry.name.en ?? entry.id;
                return <button type="button" key={entry.id} aria-disabled={entry.status !== "valid"}
                  className={`theme-card palette-card${active ? " active" : ""}${entry.status !== "valid" ? " is-invalid" : ""}`}
                  onClick={() => { if (entry.status === "valid") void selectV2Theme(entry.id); }} aria-pressed={active} title={entry.diagnostics.map((x) => x.message).join("\n") || label}>
                  <div className="palette-card-preview tailsync" aria-hidden="true"><span className="palette-preview-rail" /><span className="palette-preview-title">{entry.status === "valid" ? "V2" : "!"}</span><span className="palette-preview-row row-one" /><span className="palette-preview-row row-two" /></div>
                  <span className="palette-card-label">{label}</span>{active && <Check className="palette-card-check" size={13} strokeWidth={2} aria-hidden="true" />}
                  {(entry.source === "custom" || entry.status === "invalid") && <span className="theme-card-tools">
                    {entry.source === "custom" && entry.status === "valid" && <>
                      <span role="button" tabIndex={0} className="theme-card-tool" onClick={(event) => { event.stopPropagation(); void handleUpdateTheme(entry); }} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); void handleUpdateTheme(entry); } }} title={t("settings.customThemeUpdate")} aria-label={`${t("settings.customThemeUpdate")} ${label}`}><RefreshCw size={12} strokeWidth={1.8} aria-hidden="true" /></span>
                      <span role="button" tabIndex={0} className="theme-card-tool" onClick={(event) => { event.stopPropagation(); void rollbackV2Theme(entry); }} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); void rollbackV2Theme(entry); } }} title={t("settings.customThemeRollback")} aria-label={`${t("settings.customThemeRollback")} ${label}`}><RotateCcw size={12} strokeWidth={1.8} aria-hidden="true" /></span>
                    </>}
                    <span role="button" tabIndex={0} className="theme-card-tool custom-theme-delete" onClick={(event) => { event.stopPropagation(); void deleteV2Theme(entry); }} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); void deleteV2Theme(entry); } }} title={t("settings.customThemeDelete")} aria-label={`${t("settings.customThemeDelete")} ${label}`}><Trash2 size={12} strokeWidth={1.8} aria-hidden="true" /></span>
                  </span>}
                </button>;
              })}
            </div>
            <div className="custom-themes-actions"><button type="button" className="custom-theme-action" onClick={() => void handleImportTheme()}><Upload size={13} strokeWidth={1.8} aria-hidden="true" />{t("settings.customThemeImport")}</button></div>
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

        <section className="setting-group update-group">
          <div className="setting-group-header">
            <h3>{t("settings.updates")}</h3>
            <p>{t("settings.updatesDescription")}</p>
          </div>
          <div className="setting-row update-row">
            <div className="setting-row-info update-version">
              <span>TailSync {updateStatus?.current_version ?? "-"}</span>
              <small className={updatePhase === "error" ? "update-status error" : "update-status"}>
                {updateMessage}
              </small>
            </div>
            <button
              className="update-action"
              type="button"
              onClick={() => void (availableUpdate ? handleInstallUpdate() : handleCheckForUpdate())}
              disabled={
                updateBusy
                || updatePhase === "loading"
                || updatePhase === "disabled"
                || updatePhase === "installed"
              }
            >
              <RefreshCw
                size={15}
                strokeWidth={1.8}
                className={updateBusy ? "spin" : undefined}
                aria-hidden="true"
              />
              <span>{availableUpdate ? t("settings.updateInstall") : t("settings.updateCheck")}</span>
            </button>
          </div>
        </section>
      </div>

      <ShortcutRecorderDialog
        recorder={syncShortcutRecorder}
        title={t("settings.shortcutDialogTitle")}
        prompt={t("settings.shortcutDialogPrompt")}
        t={t}
      />
      <ShortcutRecorderDialog
        recorder={historyShortcutRecorder}
        title={t("settings.historyShortcutDialogTitle")}
        prompt={t("settings.historyShortcutDialogPrompt")}
        t={t}
      />

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
                {pairingStatus.phase === "finalizing" ? (
                  <p className="pairing-progress">{t("settings.pairingFinalizing")}</p>
                ) : pairingStatus.phase === "waiting_for_peer" && (
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
                disabled={pairingBusy || pairingStatus?.phase === "finalizing" || !pairingStatus?.peer || pairingStatus.peer.local_confirmed}
              >
                {t(pairingStatus?.peer?.local_confirmed
                  ? "settings.confirmed"
                  : "settings.codesMatch")}
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingThemeImport && (
        <div className="dialog-backdrop" onMouseDown={() => setPendingThemeImport(null)}>
          <div className="shortcut-dialog theme-import-dialog" role="dialog" aria-modal="true" aria-label={t("settings.themePreviewTitle")} onMouseDown={(event) => event.stopPropagation()}>
            <div className="shortcut-dialog-header"><div><h2>{t("settings.themePreviewTitle")}</h2></div></div>
             <div className="theme-package-preview-modes">
               {[{
                 label: t("settings.themePreviewLight"),
                 resolved: pendingThemeImport.previews.light,
               }, {
                 label: t("settings.themePreviewDark"),
                 resolved: pendingThemeImport.previews.dark,
               }, {
                 label: t("settings.themePreviewHighContrastLight"),
                 resolved: pendingThemeImport.previews.highContrastLight,
               }, {
                 label: t("settings.themePreviewHighContrastDark"),
                 resolved: pendingThemeImport.previews.highContrastDark,
               }].map(({ label, resolved }) => (
                 <ThemePackagePreview
                   key={label}
                   label={label}
                   resolved={resolved}
                   path={pendingThemeImport.path}
                   digest={pendingThemeImport.digest}
                   stateLabels={{
                     default: t("settings.themePreviewStateDefault"),
                     hover: t("settings.themePreviewStateHover"),
                     active: t("settings.themePreviewStateActive"),
                     selected: t("settings.themePreviewStateSelected"),
                     disabled: t("settings.themePreviewStateDisabled"),
                     focus: t("settings.themePreviewStateFocus"),
                   }}
                 />
               ))}
             </div>
            <div className="shortcut-dialog-message" role="status">
              {pendingThemeImport.operation.kind === "update"
                ? `${pendingThemeImport.operation.installedVersion} → ${pendingThemeImport.candidateVersion}`
                : `${t("settings.customThemeCandidateVersion")}: ${pendingThemeImport.candidateVersion}`}
            </div>
            {pendingThemeImport.diagnostics.length > 0 && <div className="shortcut-dialog-message" role="status">{pendingThemeImport.diagnostics.map((diagnostic) => <div key={`${diagnostic.code}-${diagnostic.message}`}>{diagnostic.message}</div>)}</div>}
            <div className="confirm-dialog-actions"><button type="button" onClick={() => setPendingThemeImport(null)}>{t("settings.cancel")}</button><button type="button" className="pair-submit" onClick={() => void confirmThemeImport()}>{t(pendingThemeImport.operation.kind === "install" ? "settings.customThemeInstall" : "settings.customThemeUpdate")}</button></div>
          </div>
        </div>
      )}

      {/* ── Toast ── */}
      {errorMessage ? (
        <div className="toast" role="alert">{errorMessage}</div>
      ) : saved && <div className="toast" role="status">{t("settings.saved")}</div>}
    </div>
  );
}
