import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, updateSettings, type SettingsPatch } from "../tailsyncClient";
import type { SettingsData } from "../types/settings.generated";
import { LatestRequest, SerialTaskQueue } from "../utils/asyncControl";

/** Shared save pipeline for the Settings and Connections windows. */
export function useSettingsEditor(
  setLocale: (language: SettingsData["language"]) => void,
  saveFailedMessage: string,
) {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [saved, setSaved] = useState(false);
  const [errorMessage, setErrorMessage] = useState("");
  const settingsRef = useRef<SettingsData | null>(null);
  const saveQueue = useRef(new SerialTaskQueue());
  const settingsUpdates = useRef(new LatestRequest());
  const localRevision = useRef(0);
  const toastTimer = useRef<number>(0);

  const applyCanonical = useCallback((next: SettingsData) => {
    settingsRef.current = next;
    setSettings(next);
    setLocale(next.language);
  }, [setLocale]);

  const hydrate = useCallback((next: SettingsData) => {
    if (settingsRef.current) return false;
    applyCanonical(next);
    return true;
  }, [applyCanonical]);

  const setLocalSettings = useCallback((patch: Partial<SettingsData>) => {
    const current = settingsRef.current;
    if (!current) return;
    localRevision.current += 1;
    const next = { ...current, ...patch };
    settingsRef.current = next;
    setSettings(next);
  }, []);

  const showSavedToast = useCallback(() => {
    setSaved(true);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setSaved(false), 1500);
  }, []);

  const update = useCallback(async (patch: SettingsPatch) => {
    const previous = settingsRef.current;
    if (!previous) return false;
    setErrorMessage("");
    setLocalSettings(patch);
    const generation = settingsUpdates.current.begin();
    try {
      const canonical = await saveQueue.current.enqueue(() => updateSettings(patch));
      if (settingsUpdates.current.isCurrent(generation)) {
        applyCanonical(canonical);
        showSavedToast();
      }
      return true;
    } catch (error) {
      if (settingsUpdates.current.isCurrent(generation)) {
        try {
          applyCanonical(await getSettings());
        } catch {
          applyCanonical(previous);
        }
      }
      console.error("Save settings failed:", error);
      setErrorMessage(saveFailedMessage);
      return false;
    }
  }, [applyCanonical, saveFailedMessage, setLocalSettings, showSavedToast]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen("settings-changed", () => {
      const revision = localRevision.current;
      void saveQueue.current.enqueue(getSettings).then((canonical) => {
        if (active && localRevision.current === revision) applyCanonical(canonical);
      }).catch(console.error);
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
      window.clearTimeout(toastTimer.current);
    };
  }, [applyCanonical]);

  return {
    settings,
    settingsRef,
    saved,
    errorMessage,
    setErrorMessage,
    applyCanonical,
    hydrate,
    setLocalSettings,
    showSavedToast,
    update,
  };
}
