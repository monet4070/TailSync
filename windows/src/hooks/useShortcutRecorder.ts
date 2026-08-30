// Global-shortcut recorder feature (T256 extraction from Settings.tsx).
//
// Owns the shortcut draft, the recording/capture state machine, the capture
// keyboard listeners, and the dialog focus trap. Settings persistence and
// toasts are injected through options so the feature stays isolated:
//   - currentShortcut: current persisted shortcut, or null while settings
//     are not loaded yet
//   - applyShortcut: write the new shortcut into the settings hub
//   - showSavedToast / showError: transient UI feedback

import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  resumeSyncShortcut,
  suspendSyncShortcut,
} from "../tailsyncClient";
import { captureShortcut } from "../utils/shortcut";
import { useI18n } from "./useI18n";

export const DEFAULT_SYNC_SHORTCUT = "CommandOrControl+Shift+S";
export const DEFAULT_HISTORY_SHORTCUT = "CommandOrControl+Shift+H";

export interface ShortcutRecorderOptions {
  defaultShortcut: string;
  currentShortcut: () => string | null;
  setShortcut: (shortcut: string) => Promise<void>;
  applyShortcut: (shortcut: string) => void;
  showSavedToast: () => void;
  showError: (message: string) => void;
}

export function useShortcutRecorder(
  options: ShortcutRecorderOptions,
) {
  const { t } = useI18n();
  const {
    defaultShortcut,
    currentShortcut,
    setShortcut,
    applyShortcut,
    showSavedToast,
    showError,
  } = options;
  const [shortcutDraft, setShortcutDraft] = useState(defaultShortcut);
  const [shortcutBusy, setShortcutBusy] = useState(false);
  const [shortcutRecording, setShortcutRecording] = useState(false);
  const [shortcutCandidate, setShortcutCandidate] = useState("");
  const [shortcutPreviewKeys, setShortcutPreviewKeys] = useState<string[]>([]);
  const [shortcutCaptureActive, setShortcutCaptureActive] = useState(false);
  const [shortcutDialogError, setShortcutDialogError] = useState("");
  const shortcutTriggerRef = useRef<HTMLButtonElement>(null);
  const shortcutDialogRef = useRef<HTMLDivElement>(null);
  const shortcutCaptureRef = useRef<HTMLButtonElement>(null);
  const shortcutPreviousFocus = useRef<HTMLElement | null>(null);
  const shortcutRecordingRef = useRef(false);

  // Unmount: restore the shortcut if recording was abandoned.
  useEffect(() => () => {
    if (shortcutRecordingRef.current) void resumeSyncShortcut();
  }, []);

  const commitShortcut = useCallback(
    async (
      nextShortcut = shortcutDraft,
      forceRegistration = false,
      reportInline = false,
    ) => {
      const current = currentShortcut();
      if (current === null) return false;
      const shortcut = nextShortcut.trim();
      if (!forceRegistration && shortcut === current) return true;
      setShortcutBusy(true);
      showError("");
      try {
        await setShortcut(shortcut);
        applyShortcut(shortcut);
        setShortcutDraft(shortcut);
        showSavedToast();
        return true;
      } catch (error) {
        console.error("Shortcut registration failed:", error);
        await resumeSyncShortcut().catch(console.error);
        setShortcutDraft(current);
        if (!reportInline) showError(t("settings.shortcutConflict"));
        return false;
      } finally {
        setShortcutBusy(false);
      }
    },
    [currentShortcut, shortcutDraft, setShortcut, applyShortcut, showSavedToast, showError, t],
  );

  const startShortcutRecording = async () => {
    if (shortcutBusy || shortcutRecording) return;
    setShortcutBusy(true);
    showError("");
    try {
      await suspendSyncShortcut();
      shortcutRecordingRef.current = true;
      shortcutPreviousFocus.current = document.activeElement as HTMLElement | null;
      setShortcutCandidate("");
      setShortcutPreviewKeys([]);
      setShortcutDialogError("");
      setShortcutCaptureActive(true);
      setShortcutRecording(true);
    } catch (error) {
      console.error("Could not start shortcut recording:", error);
      showError(t("settings.shortcutConflict"));
    } finally {
      setShortcutBusy(false);
    }
  };

  const cancelShortcutRecording = async () => {
    shortcutRecordingRef.current = false;
    setShortcutRecording(false);
    setShortcutCandidate("");
    setShortcutPreviewKeys([]);
    setShortcutDialogError("");
    setShortcutCaptureActive(false);
    setShortcutDraft(currentShortcut() ?? defaultShortcut);
    try {
      await resumeSyncShortcut();
    } catch (error) {
      console.error("Could not restore shortcut:", error);
      showError(t("settings.shortcutConflict"));
    }
  };

  const restartShortcutCapture = () => {
    setShortcutCandidate("");
    setShortcutPreviewKeys([]);
    setShortcutDialogError("");
    setShortcutCaptureActive(true);
    window.requestAnimationFrame(() => shortcutCaptureRef.current?.focus());
  };

  const confirmShortcut = async () => {
    if (!shortcutCandidate || shortcutBusy) return;
    const success = await commitShortcut(shortcutCandidate, true, true);
    if (success) {
      shortcutRecordingRef.current = false;
      setShortcutRecording(false);
      setShortcutCaptureActive(false);
      setShortcutDialogError("");
      return;
    }

    setShortcutDialogError(t("settings.shortcutConflict"));
    try {
      await suspendSyncShortcut();
      shortcutRecordingRef.current = true;
    } catch (error) {
      console.error("Could not keep shortcut capture isolated:", error);
      shortcutRecordingRef.current = false;
      setShortcutRecording(false);
      showError(t("settings.shortcutConflict"));
    }
  };

  const handleShortcutCaptureEvent = (
    event: KeyboardEvent | ReactKeyboardEvent<HTMLButtonElement>,
  ) => {
    if (!shortcutRecording || !shortcutCaptureActive || event.repeat) return;
    if (
      event.key === "Escape"
      && !event.ctrlKey
      && !event.altKey
      && !event.shiftKey
      && !event.metaKey
    ) return;

    event.preventDefault();
    event.stopPropagation();
    const result = captureShortcut(event);
    setShortcutPreviewKeys(result.keycaps);
    if (result.kind === "modifier") {
      setShortcutDialogError("");
    } else if (result.kind === "invalid") {
      setShortcutCandidate("");
      setShortcutDialogError(t(
        result.reason === "modifier-required"
          ? "settings.shortcutModifierRequired"
          : "settings.shortcutUnsupported",
      ));
    } else {
      setShortcutCandidate(result.shortcut);
      setShortcutDialogError("");
      setShortcutCaptureActive(false);
    }
  };
  const cancelShortcut = useEffectEvent(cancelShortcutRecording);
  const captureShortcutEvent = useEffectEvent(handleShortcutCaptureEvent);

  useEffect(() => {
    if (!shortcutRecording) return;
    const dialog = shortcutDialogRef.current;
    if (!dialog) return;
    const focusableSelector = [
      "button:not([disabled])",
      "[href]",
      "input:not([disabled])",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");
    const frame = window.requestAnimationFrame(() => shortcutCaptureRef.current?.focus());
    const handleDialogKeyDown = (event: KeyboardEvent) => {
      if (
        event.key === "Escape"
        && !event.ctrlKey
        && !event.altKey
        && !event.shiftKey
        && !event.metaKey
      ) {
        event.preventDefault();
        event.stopPropagation();
        void cancelShortcut();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...dialog.querySelectorAll<HTMLElement>(focusableSelector)];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleDialogKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleDialogKeyDown);
      shortcutPreviousFocus.current?.focus();
      shortcutPreviousFocus.current = null;
    };
  }, [shortcutRecording]);

  useEffect(() => {
    if (!shortcutRecording || !shortcutCaptureActive) return;
    const handleShortcutKeyUp = (event: KeyboardEvent) => {
      if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(event.key)) {
        setShortcutPreviewKeys([]);
      }
    };
    const handleShortcutKeyDown = (event: KeyboardEvent) => {
      captureShortcutEvent(event);
    };
    window.addEventListener("keydown", handleShortcutKeyDown, true);
    window.addEventListener("keyup", handleShortcutKeyUp, true);
    return () => {
      window.removeEventListener("keydown", handleShortcutKeyDown, true);
      window.removeEventListener("keyup", handleShortcutKeyUp, true);
    };
  }, [shortcutRecording, shortcutCaptureActive]);

  return {
    shortcutDraft,
    setShortcutDraft,
    shortcutBusy,
    shortcutRecording,
    shortcutCandidate,
    shortcutPreviewKeys,
    shortcutCaptureActive,
    shortcutDialogError,
    shortcutTriggerRef,
    shortcutDialogRef,
    shortcutCaptureRef,
    commitShortcut,
    startShortcutRecording,
    cancelShortcutRecording,
    restartShortcutCapture,
    confirmShortcut,
    handleShortcutCaptureEvent,
  };
}
