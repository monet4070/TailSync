import { useCallback, useState, useEffect, useRef } from "react";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { LatestRequest, SerialTaskQueue } from "../utils/asyncControl";
import type { SettingsData } from "../types/settings.generated";
import {
  closeConnectionsWindow,
  forgetPeer,
  getSettings,
  updateSettings,
  type PeerDevice,
} from "../tailsyncClient";
import { useConnectionTests } from "../hooks/useConnectionTests";
import { useDevices } from "../hooks/useDevices";
import { usePairing } from "../hooks/usePairing";
import { useRemotePairing } from "../hooks/useRemotePairing";
import { X } from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";
import { SettingsConnectionsSection } from "./settings/SettingsConnectionsSection";
import { PairingDialog } from "./settings/SettingsDialogs";

export function Connections() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [saved, setSaved] = useState(false);
  const [errorMessage, setErrorMessage] = useState("");
  const { theme } = useTheme();
  const { t, setLocale } = useI18n();
  const toastTimer = useRef<number>(0);
  const settingsRef = useRef<SettingsData | null>(null);
  const saveQueue = useRef(new SerialTaskQueue());
  const settingsUpdates = useRef(new LatestRequest());

  const applyPeerEnabled = useCallback((hostname: string, enabled: boolean) => {
    setSettings((current) =>
      current
        ? {
            ...current,
            enabled_peers: { ...current.enabled_peers, [hostname]: enabled },
          }
        : current,
    );
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

  useEffect(() => {
    getSettings()
      .then((s) => {
        settingsRef.current = s;
        setSettings(s);
        setLocale(s.language);
      })
      .catch(console.error);
  }, [setLocale]);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  const update = async (patch: Partial<SettingsData>) => {
    const previous = settingsRef.current;
    if (!previous) return false;
    const next = { ...previous, ...patch };
    setErrorMessage("");
    settingsRef.current = next;
    setSettings(next);
    const generation = settingsUpdates.current.begin();
    const save = saveQueue.current.enqueue(() => updateSettings(next));
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
        } catch {
          settingsRef.current = previous;
          setSettings(previous);
        }
      }
      console.error("Save settings failed:", e);
      setErrorMessage(t("settings.saveFailed"));
      return false;
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

  const remotePairing = useRemotePairing();

  const handleConnectionMode = async (mode: SettingsData["connection_mode"]) => {
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

  const appClassName = `app settings-window connections-window ${theme}`;

  if (!settings) {
    return (
      <div className={appClassName}>
        <div className="titlebar" data-tauri-drag-region>
          <div className="titlebar-brand">
            <ThemeLogo />
            <span className="titlebar-text">{t("connections.title")}</span>
            <span className="titlebar-badge">v2</span>
          </div>
          <button
            className="titlebar-close"
            onClick={() => void closeConnectionsWindow()}
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
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <ThemeLogo />
          <span className="titlebar-text">{t("connections.title")}</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => void closeConnectionsWindow()}
          title={t("settings.closePairing")}
          aria-label={t("settings.closePairing")}
        >
          <X size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>

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
          remotePairing={remotePairing}
          defaultExpandedRemotePairing={false}
        />
      </div>

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

      {errorMessage ? (
        <div className="toast" role="alert">
          {errorMessage}
        </div>
      ) : (
        saved && <div className="toast" role="status">{t("settings.saved")}</div>
      )}
    </div>
  );
}
