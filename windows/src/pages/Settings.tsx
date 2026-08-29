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
  type StorageMigrationResult,
  type StorageStatus,
  type ThemeV2Descriptor,
} from "../tailsyncClient";
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
import { X } from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";
import { GIB } from "./settings/SettingsFormatters";
import { ShortcutRecorderDialog } from "./settings/SettingsShortcutControls";
import { SettingsConnectionsSection } from "./settings/SettingsConnectionsSection";
import { SettingsGeneralSection } from "./settings/SettingsGeneralSection";
import { SettingsHistorySection } from "./settings/SettingsHistorySection";
import { SettingsStorageSection } from "./settings/SettingsStorageSection";
import { SettingsAppearanceSection } from "./settings/SettingsAppearanceSection";
import { SettingsUpdateSection } from "./settings/SettingsUpdateSection";
import { PairingDialog, ThemeImportDialog } from "./settings/SettingsDialogs";

/* ── Types ──────────────────────────────────────────────────────── */

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

  const changeLanguage = async (language: SettingsData["language"]) => {
    const savedLanguage = await update({ language });
    if (!savedLanguage) {
      setLocale(settingsRef.current?.language ?? settings?.language ?? language);
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
        <SettingsConnectionsSection
          settings={settings}
          t={t}
          devices={devices}
          devicesLoading={devicesLoading}
          devicesError={devicesError}
          pairingStatus={pairingStatus}
          pairingBusy={pairingBusy}
          connectionTests={connectionTests}
          refreshDevices={refreshDevices}
          handleConnectionMode={handleConnectionMode}
          closePairing={closePairing}
          handleEnablePairing={handleEnablePairing}
          handleTestConnection={handleTestConnection}
          handlePeerToggle={handlePeerToggle}
          handleForget={handleForget}
          openPairing={openPairing}
        />
        <SettingsGeneralSection
          settings={settings}
          t={t}
          syncShortcutRecorder={syncShortcutRecorder}
          historyShortcutRecorder={historyShortcutRecorder}
          setGlobalSync={setGlobalSync}
          update={update}
        />
        <SettingsHistorySection
          t={t}
          historyLimitDraft={historyLimitDraft}
          setHistoryLimitDraft={setHistoryLimitDraft}
          commitHistoryLimit={commitHistoryLimit}
        />
        <SettingsStorageSection
          settings={settings}
          t={t}
          storageStatus={storageStatus}
          storageBusy={storageBusy}
          storageQuotaDraft={storageQuotaDraft}
          oldStorage={oldStorage}
          setStorageQuotaDraft={setStorageQuotaDraft}
          setOldStorage={setOldStorage}
          changeStorage={changeStorage}
          commitStorageQuota={commitStorageQuota}
          handleDeleteOldStorage={handleDeleteOldStorage}
        />
        <SettingsAppearanceSection
          settings={settings}
          t={t}
          locale={locale}
          themePreference={themePreference}
          v2Themes={v2Themes}
          v2Active={v2Active}
          setLocale={setLocale}
          changeThemePreference={handleThemeChange}
          selectV2Theme={selectV2Theme}
          handleImportTheme={handleImportTheme}
          handleUpdateTheme={handleUpdateTheme}
          rollbackV2Theme={rollbackV2Theme}
          deleteV2Theme={deleteV2Theme}
          changeLanguage={changeLanguage}
        />
        <SettingsUpdateSection
          t={t}
          updateStatus={updateStatus}
          updatePhase={updatePhase}
          availableUpdate={availableUpdate}
          updateMessage={updateMessage}
          updateBusy={updateBusy}
          handleCheckForUpdate={handleCheckForUpdate}
          handleInstallUpdate={handleInstallUpdate}
        />
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

      <PairingDialog
        t={t}
        pairingOpen={pairingOpen}
        pairingStatus={pairingStatus}
        pairingTarget={pairingTarget}
        pairingError={pairingError}
        pairingBusy={pairingBusy}
        pairDialogRef={pairDialogRef}
        closePairing={closePairing}
        handlePair={handlePair}
      />

      <ThemeImportDialog
        t={t}
        pendingThemeImport={pendingThemeImport}
        setPendingThemeImport={setPendingThemeImport}
        confirmThemeImport={confirmThemeImport}
      />

      {/* ── Toast ── */}
      {errorMessage ? (
        <div className="toast" role="alert">{errorMessage}</div>
      ) : saved && <div className="toast" role="status">{t("settings.saved")}</div>}
    </div>
  );
}
