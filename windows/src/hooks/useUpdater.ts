// Updater feature state and actions (T248 extraction from Settings.tsx).
//
// Owns the update-status polling result, the available-update info, the
// check/install state machine, and the derived UI message. The feature is
// fully isolated: it shares no state with the rest of Settings.

import { useEffect, useState } from "react";
import {
  checkForUpdate,
  getUpdateStatus,
  installUpdate,
  type UpdateInfo,
  type UpdateStatus,
} from "../tailsyncClient";
import { useI18n } from "./useI18n";

export type UpdatePhase =
  | "loading"
  | "idle"
  | "checking"
  | "available"
  | "current"
  | "disabled"
  | "installing"
  | "installed"
  | "error";

export function useUpdater() {
  const { t } = useI18n();
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [availableUpdate, setAvailableUpdate] = useState<UpdateInfo | null>(null);
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>("loading");
  const [updateError, setUpdateError] = useState("");

  // Initial load, mirroring the previous Settings init effect.
  useEffect(() => {
    let cancelled = false;
    getUpdateStatus()
      .then((status) => {
        if (cancelled) return;
        setUpdateStatus(status);
        setUpdatePhase(status.updates_enabled ? "idle" : "disabled");
      })
      .catch((error) => {
        if (cancelled) return;
        setUpdateError(String(error));
        setUpdatePhase("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleCheckForUpdate = async () => {
    if (!updateStatus?.updates_enabled) {
      setUpdatePhase("disabled");
      return;
    }
    setUpdatePhase("checking");
    setUpdateError("");
    try {
      const result = await checkForUpdate();
      setAvailableUpdate(result);
      setUpdatePhase(result ? "available" : "current");
    } catch (error) {
      setUpdateError(String(error));
      setUpdatePhase("error");
    }
  };

  const handleInstallUpdate = async () => {
    if (!availableUpdate) return;
    setUpdatePhase("installing");
    setUpdateError("");
    try {
      const installed = await installUpdate();
      setUpdatePhase(installed ? "installed" : "current");
      if (!installed) setAvailableUpdate(null);
    } catch (error) {
      setUpdateError(String(error));
      setUpdatePhase("error");
    }
  };

  const updateMessage = (() => {
    switch (updatePhase) {
      case "loading": return t("settings.updateLoading");
      case "checking": return t("settings.updateChecking");
      case "available": return t("settings.updateAvailable")
        .replace("{version}", availableUpdate?.version ?? "");
      case "current": return t("settings.updateCurrent");
      case "disabled": return t("settings.updateDisabled");
      case "installing": return t("settings.updateInstalling");
      case "installed": return t("settings.updateInstalled");
      case "error": return updateError || t("settings.updateFailed");
      default: return t("settings.updateReady");
    }
  })();

  const updateBusy = updatePhase === "checking" || updatePhase === "installing";

  return {
    updateStatus,
    availableUpdate,
    updatePhase,
    updateError,
    updateMessage,
    updateBusy,
    handleCheckForUpdate,
    handleInstallUpdate,
  };
}
