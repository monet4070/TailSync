import { useCallback, useEffect } from "react";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import type { SettingsData } from "../types/settings.generated";
import {
  closeConnectionsWindow,
  forgetPeer,
  getSettings,
  type PeerDevice,
} from "../tailsyncClient";
import { useSettingsEditor } from "../hooks/useSettingsEditor";
import { useConnectionTests } from "../hooks/useConnectionTests";
import { useDevices } from "../hooks/useDevices";
import { usePairing } from "../hooks/usePairing";
import { useRemotePairing } from "../hooks/useRemotePairing";
import { X } from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";
import { SettingsConnectionsSection } from "./settings/SettingsConnectionsSection";
import { PairingDialog } from "./settings/SettingsDialogs";

export function Connections() {
  const { theme } = useTheme();
  const { t, setLocale } = useI18n();
  const { settings, settingsRef, saved, errorMessage, hydrate, setLocalSettings, update } =
    useSettingsEditor(setLocale, t("settings.saveFailed"));

  const applyPeerEnabled = useCallback((hostname: string, enabled: boolean) => {
    const current = settingsRef.current;
    if (!current) return;
    setLocalSettings({
      enabled_peers: { ...current.enabled_peers, [hostname]: enabled },
    });
  }, [settingsRef, setLocalSettings]);

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
        hydrate(s);
      })
      .catch(console.error);
  }, [hydrate]);

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
