import { GuidedTour } from "../onboarding/GuidedTour";
import { useCallback, useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme, type ThemePreference } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { useSettingsEditor } from "../hooks/useSettingsEditor";
import type { SettingsData } from "../types/settings.generated";
import {
  changeStorageLocation,
  closeSettingsWindow,
  deleteOldStorage,
  deleteThemeV2,
  formatThemeError,
  getSettings,
  getStorageStatus,
  setHistoryShortcut,
  installThemeV2,
  listThemesV2,
  getLocalThemeSettingsV2,
  setLocalThemeSettingsV2,
  openConnectionsWindow,
  rollbackThemeV2,
  setSyncEnabled,
  setSyncShortcut,
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
import {
  DEFAULT_HISTORY_SHORTCUT,
  DEFAULT_SYNC_SHORTCUT,
  useShortcutRecorder,
} from "../hooks/useShortcutRecorder";
import { useUpdater } from "../hooks/useUpdater";
import { useLaunchAtLogin } from "../hooks/useLaunchAtLogin";
import { X, ArrowUpRight } from "lucide-react";
import { readTourSuppressed, startTour, TOUR_SUPPRESSED_EVENT } from "../onboarding/tourState";
import onboardingGuide from "../../../shared/onboarding/guide.json";
import "../onboarding/onboarding.css";
import { ThemeLogo } from "../ThemeLogo";
import { GIB } from "./settings/SettingsFormatters";
import { ShortcutRecorderDialog } from "./settings/SettingsShortcutControls";
import { SettingsGeneralSection } from "./settings/SettingsGeneralSection";
import { SettingsHistorySection } from "./settings/SettingsHistorySection";
import { SettingsStorageSection } from "./settings/SettingsStorageSection";
import { SettingsAppearanceSection } from "./settings/SettingsAppearanceSection";
import { SettingsUpdateSection } from "./settings/SettingsUpdateSection";
import { ThemeImportDialog } from "./settings/SettingsDialogs";

/* ── Types ──────────────────────────────────────────────────────── */

export function Settings() {
  const [tourSuppressed, setTourSuppressed] = useState(false);
  const [v2Themes, setV2Themes] = useState<ThemeV2Descriptor[]>([]);
  const [v2Active, setV2Active] = useState("builtin:canvas@1");
  const [v2HighContrast, setV2HighContrast] = useState(false);
  const [pendingThemeImport, setPendingThemeImport] = useState<PendingThemePackage | null>(null);
  const [historyLimitDraft, setHistoryLimitDraft] = useState(100);
  const [storageQuotaDraft, setStorageQuotaDraft] = useState("10");
  const [storageStatus, setStorageStatus] = useState<StorageStatus | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [oldStorage, setOldStorage] = useState<StorageMigrationResult | null>(null);
  const {
    theme,
    setTheme,
    themePreference,
  } = useTheme();
  const { t, setLocale, locale } = useI18n();
  useEffect(() => {
    const sync = () => { void readTourSuppressed().then(value => setTourSuppressed(value === true)).catch(() => setTourSuppressed(false)); };
    sync();
    window.addEventListener(TOUR_SUPPRESSED_EVENT, sync);
    return () => window.removeEventListener(TOUR_SUPPRESSED_EVENT, sync);
  }, []);
  const launchAtLogin = useLaunchAtLogin();
  const {
    settings,
    settingsRef,
    saved,
    errorMessage,
    setErrorMessage,
    showSavedToast,
    update,
    installLocal,
  } = useSettingsEditor({ onLocale: setLocale, translate: t });
  const currentShortcut = useCallback(
    () => settingsRef.current?.sync_shortcut ?? null,
    [settingsRef],
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
    [settingsRef],
  );
  // Dedicated commands own these fields; the local draft records them so an
  // older generic response cannot roll them back.
  const applyShortcut = useCallback(
    (shortcut: string) => installLocal({ sync_shortcut: shortcut }),
    [installLocal],
  );
  const applyHistoryShortcut = useCallback(
    (shortcut: string) => installLocal({ history_shortcut: shortcut }),
    [installLocal],
  );
  const showError = useCallback((message: string) => setErrorMessage(message), [setErrorMessage]);
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
  const {
    updateStatus,
    availableUpdate,
    updatePhase,
    updateMessage,
    updateBusy,
    handleCheckForUpdate,
    handleInstallUpdate,
  } = useUpdater();

  // Follow refreshed settings where the input is still unchanged. Keep edits
  // made independently of the last settings snapshot, including an ongoing drag.
  const numericSettingsSnapshot = useRef<{ history: number; quota: string } | null>(null);
  useEffect(() => {
    if (!settings) return;
    const previous = numericSettingsSnapshot.current;
    const quota = String(Math.round(settings.storage_quota_bytes / GIB));
    numericSettingsSnapshot.current = { history: settings.history_limit, quota };
    setHistoryLimitDraft((draft) => !previous || draft === previous.history ? settings.history_limit : draft);
    setStorageQuotaDraft((draft) => !previous || draft === previous.quota ? quota : draft);
    if (!previous) {
      setSyncShortcutDraft(settings.sync_shortcut);
      setHistoryShortcutDraft(settings.history_shortcut);
    }
  }, [settings, setSyncShortcutDraft, setHistoryShortcutDraft]);

  useEffect(() => {
    getStorageStatus().then(setStorageStatus).catch(console.error);
  }, []);
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<{ enabled: boolean }>("sync-state-changed", ({ payload }) => {
      if (!active) return;
      const current = settingsRef.current;
      if (!current || current.sync_enabled === payload.enabled) return;
      installLocal({ sync_enabled: payload.enabled });
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [installLocal, settingsRef]);

  const commitHistoryLimit = async () => {
    if (historyLimitDraft === settingsRef.current?.history_limit) return;
    if (!(await update({ history_limit: historyLimitDraft }))) {
      const persisted = settingsRef.current?.history_limit ?? historyLimitDraft;
      setHistoryLimitDraft((draft) => draft === historyLimitDraft ? persisted : draft);
    }
  };

  const setAutoSaveNetworkImages = async (enabled: boolean) => {
    await update({ auto_save_network_images: enabled });
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
      const persisted = String(Math.round(
        (settingsRef.current?.storage_quota_bytes ?? current.storage_quota_bytes) / GIB,
      ));
      setStorageQuotaDraft((draft) => draft === String(gib) ? persisted : draft);
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
      installLocal(canonical);
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
    installLocal({ sync_enabled: enabled });
    try {
      await setSyncEnabled(enabled);
    } catch (error) {
      console.error("Could not change sync state:", error);
      installLocal({ sync_enabled: current.sync_enabled });
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
      <GuidedTour window="settings" />
      <div className="settings-content">
        {!tourSuppressed && <button type="button" className="onboarding-entry" onClick={() => {
          void startTour().catch(() => setErrorMessage(t("settings.error")));
        }}>
          <span><strong>{onboardingGuide.ui.title[locale.startsWith("zh") ? "zh-CN" : "en"]}</strong><small>{onboardingGuide.ui.reopen[locale.startsWith("zh") ? "zh-CN" : "en"]}</small></span>
          <ArrowUpRight size={18} aria-hidden="true" />
        </button>}
        <SettingsGeneralSection
          settings={settings}
          t={t}
          launchAtLogin={launchAtLogin}
          syncShortcutRecorder={syncShortcutRecorder}
          historyShortcutRecorder={historyShortcutRecorder}
          setGlobalSync={setGlobalSync}
          update={update}
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
        <SettingsHistorySection
          t={t}
          historyLimitDraft={historyLimitDraft}
          setHistoryLimitDraft={setHistoryLimitDraft}
          commitHistoryLimit={commitHistoryLimit}
          autoSaveNetworkImages={settings.auto_save_network_images ?? false}
          setAutoSaveNetworkImages={setAutoSaveNetworkImages}
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
        <section className="setting-group">
          <div className="setting-group-header">
            <div>
              <h3>{t("settings.connectionsTitle")}</h3>
              <p>{t("settings.connectionsDescription")}</p>
            </div>
          </div>
          <div className="setting-row">
            <div className="setting-row-info">
              <span>{t("settings.manageConnections")}</span>
              <small>{t("settings.manageConnectionsDescription")}</small>
            </div>
            <button
              type="button"
              className="pair-device-action"
              onClick={() => void openConnectionsWindow()}
            >
              {t("settings.openConnections")}
            </button>
          </div>
        </section>
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
