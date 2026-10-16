// Shared settings editor for the Settings and Connections windows.
//
// Both windows show the same server-owned settings. Each window keeps its own
// optimistic draft and sends only the fields it changed, so a save from one
// window can never roll back an unrelated field the other window edited. The
// daemon answers with the canonical snapshot, which is installed only while
// this window's draft is still the one that asked for it.

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, updateSettings } from "../tailsyncClient";
import type { SettingsData, SettingsPatch } from "../types/settings.generated";
import { LatestRequest, SerialTaskQueue } from "../utils/asyncControl";

export interface SettingsEditorOptions {
  /** Applied whenever a canonical snapshot arrives with a different locale. */
  onLocale?: (locale: SettingsData["language"]) => void;
  /** Localizes the message shown when a save fails. */
  translate?: (key: string) => string;
}

export interface SettingsEditor {
  settings: SettingsData | null;
  settingsRef: React.RefObject<SettingsData | null>;
  saved: boolean;
  errorMessage: string;
  setErrorMessage: (message: string) => void;
  showSavedToast: () => void;
  /** Send field intents and install the canonical answer when still current. */
  update: (patch: SettingsPatch) => Promise<boolean>;
  /**
   * Record a local change that a dedicated command owns (sync state, global
   * shortcuts). It becomes part of the draft so an older save response or a
   * stale canonical refetch cannot roll it back.
   */
  installLocal: (patch: Partial<SettingsData>) => void;
  /** Re-read the canonical snapshot, keeping any newer local draft. */
  refresh: () => Promise<SettingsData | null>;
}

export function useSettingsEditor(options: SettingsEditorOptions = {}): SettingsEditor {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [saved, setSaved] = useState(false);
  const [errorMessage, setErrorMessage] = useState("");
  const settingsRef = useRef<SettingsData | null>(null);
  const saveQueue = useRef(new SerialTaskQueue());
  const updates = useRef(new LatestRequest());
  const reads = useRef(new LatestRequest());
  const refreshPending = useRef(false);
  // Every local draft change bumps this, so an answer that was already in
  // flight when the user edited again cannot install itself over the new draft.
  const localRevision = useRef(0);
  const pendingSaves = useRef(0);
  const active = useRef(true);
  const toastTimer = useRef(0);
  const onLocale = useRef(options.onLocale);
  const translate = useRef(options.translate);
  // Keep the latest callbacks without reading a ref during render.
  useEffect(() => {
    onLocale.current = options.onLocale;
    translate.current = options.translate;
  });

  const installCanonical = useCallback((canonical: SettingsData) => {
    settingsRef.current = canonical;
    setSettings(canonical);
    onLocale.current?.(canonical.language);
  }, []);

  const showSavedToast = useCallback(() => {
    setSaved(true);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setSaved(false), 1500);
  }, []);

  const installLocal = useCallback((patch: Partial<SettingsData>) => {
    const current = settingsRef.current;
    if (!current) return;
    localRevision.current += 1;
    reads.current.invalidate();
    settingsRef.current = { ...current, ...patch };
    setSettings(settingsRef.current);
  }, []);

  const refresh = useCallback(async (): Promise<SettingsData | null> => {
    if (!active.current) return null;
    if (pendingSaves.current > 0) {
      refreshPending.current = true;
      return null;
    }
    refreshPending.current = false;
    const generation = reads.current.begin();
    const revision = localRevision.current;
    try {
      const canonical = await getSettings();
      // Re-check after the await: an edit made while this refetch was in
      // flight, or a save still queued, owns the draft instead.
      if (active.current && reads.current.isCurrent(generation) &&
        localRevision.current === revision && pendingSaves.current === 0) {
        installCanonical(canonical);
      }
      return canonical;
    } catch (error) {
      console.error("Could not refresh settings:", error);
      return null;
    }
  }, [installCanonical]);

  useEffect(() => {
    active.current = true;
    const currentReads = reads.current;
    return () => {
      active.current = false;
      currentReads.invalidate();
      window.clearTimeout(toastTimer.current);
    };
  }, []);

  useEffect(() => {
    const generation = reads.current.begin();
    const revision = localRevision.current;
    getSettings().then((canonical) => {
      if (active.current && reads.current.isCurrent(generation) &&
        localRevision.current === revision && pendingSaves.current === 0) {
        installCanonical(canonical);
      }
    }).catch(console.error);
  }, [installCanonical]);

  // Another window changed settings: converge on the canonical snapshot, but
  // never overwrite a draft that still has a local edit or save in flight.
  useEffect(() => {
    let listening = true;
    let unlisten: (() => void) | undefined;
    void listen("settings-changed", () => {
      void refresh();
    }).then((stop) => {
      if (listening) unlisten = stop;
      else stop();
    }).catch((error: unknown) => {
      console.error("Could not listen for settings changes:", error);
    });
    return () => {
      listening = false;
      unlisten?.();
    };
  }, [refresh]);

  const update = useCallback(
    async (patch: SettingsPatch): Promise<boolean> => {
      const previous = settingsRef.current;
      if (!previous) return false;
      localRevision.current += 1;
      reads.current.invalidate();
      const revision = localRevision.current;
      const draft: SettingsData = { ...previous, ...patch };
      setErrorMessage("");
      settingsRef.current = draft;
      setSettings(draft);

      const generation = updates.current.begin();
      pendingSaves.current += 1;
      const stillCurrent = () =>
        active.current &&
        updates.current.isCurrent(generation) &&
        localRevision.current === revision;
      try {
        const canonical = await saveQueue.current.enqueue(() => updateSettings(patch));
        // Re-check after the await: a newer edit, a dedicated command, an
        // event refresh, or an unmount must win over this older response.
        if (!stillCurrent()) return true;
        installCanonical(canonical);
        showSavedToast();
        return true;
      } catch (error) {
        console.error("Save settings failed:", error);
        // The daemon may have rejected the patch, so converge on what it
        // actually holds. Only a still-current, unedited draft is replaced.
        let canonical: SettingsData | null = null;
        try {
          canonical = await getSettings();
        } catch {
          canonical = null;
        }
        if (!stillCurrent()) return false;
        if (canonical) installCanonical(canonical);
        else {
          settingsRef.current = previous;
          setSettings(previous);
        }
        setErrorMessage((translate.current ?? ((key: string) => key))("settings.saveFailed"));
        return false;
      } finally {
        pendingSaves.current -= 1;
        // A change notification may have preceded this command's reply. Read
        // after the entire queue settles, even if no second event arrives.
        if (pendingSaves.current === 0 && refreshPending.current && active.current) {
          void refresh();
        }
      }
    },
    [installCanonical, showSavedToast, refresh],
  );

  return {
    settings,
    settingsRef,
    saved,
    errorMessage,
    setErrorMessage,
    showSavedToast,
    update,
    installLocal,
    refresh,
  };
}
