import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "../hooks/useI18n";
import { useTheme } from "../hooks/useTheme";
import {
  closePreviewWindow,
  closeHistoryWindow,
  closeFavoritesWindow,
  presentPreviewWindow,
  getPreviewWindowRequest,
  restoreEntry,
  type PreviewWindowSnapshot,
} from "../tailsyncClient";
import { useRestoreActivity } from "../hooks/useRestoreActivity";
import { HistoryImeGuard } from "../utils/historyKeyboard";
import { PreviewShell } from "../preview/PreviewShell";
import {
  usePreviewPayload,
  type PreviewFailure,
  type PreviewTarget,
} from "../preview/usePreviewPayload";
import {
  usePreviewWindowFrame,
} from "../preview/usePreviewWindowFrame";

const PREVIEW_REQUEST_EVENT = "tailsync://preview-request";
const PREVIEW_CLOSE_EVENT = "tailsync://preview-close";

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(
    target.closest("button, a, input, textarea, select, [contenteditable], [role='button']"),
  );
}

function validSnapshot(snapshot: PreviewWindowSnapshot | null): snapshot is PreviewWindowSnapshot {
  return snapshot !== null &&
    Number.isSafeInteger(snapshot.revision) && snapshot.revision > 0 &&
    Number.isSafeInteger(snapshot.entryId) && snapshot.entryId > 0 &&
    (snapshot.batchId === null || typeof snapshot.batchId === "string") &&
    (snapshot.owner === "history" || snapshot.owner === "favorites") &&
    typeof snapshot.takeFocus === "boolean";
}

export function Preview() {
  const { theme } = useTheme();
  const { t } = useI18n();
  const [target, setTarget] = useState<PreviewTarget | null>(null);
  const [renderFailure, setRenderFailure] = useState<PreviewFailure | null>(null);
  const latestRevision = useRef(0);
  const [snapshot, setSnapshot] = useState<PreviewWindowSnapshot | null>(null);
  const restoring = useRef(false);
  const [localRestoring, setLocalRestoring] = useState(false);
  const restoreGeneration = useRef(0);
  const { active: externalRestoring, activeRef: externalRestoringRef } = useRestoreActivity((activity) => {
    restoreGeneration.current++;
    if (activity.previewRevision !== undefined) {
      latestRevision.current = Math.max(latestRevision.current, activity.previewRevision);
    }
  });
  const ime = useRef(new HistoryImeGuard());
  const [restoreSucceeded, setRestoreSucceeded] = useState(false);
  const [restoreFailed, setRestoreFailed] = useState(false);
  const { state, retry } = usePreviewPayload(target);

  const applySnapshot = useCallback((snapshot: PreviewWindowSnapshot | null) => {
    if (!validSnapshot(snapshot) || snapshot.revision <= latestRevision.current) return;
    latestRevision.current = snapshot.revision;
    // Consume the revision without changing the target. It must not be replayed
    // after a failed restore or Ctrl+Enter success.
    if (restoring.current || externalRestoringRef.current) return;
    setRestoreFailed(false);
    setRestoreSucceeded(false);
    setSnapshot(snapshot);
    setRenderFailure(null);
    setTarget({ entryId: snapshot.entryId, batchId: snapshot.batchId });
  }, [externalRestoringRef]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<PreviewWindowSnapshot>(PREVIEW_REQUEST_EVENT, (event) => {
      if (!disposed) applySnapshot(event.payload);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    const generation = restoreGeneration.current;
    void getPreviewWindowRequest().then((snapshot) => {
      if (!disposed && generation === restoreGeneration.current) applySnapshot(snapshot);
    }).catch((error: unknown) => {
      console.error("Could not read the preview-window request:", error);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applySnapshot]);

  const payload = state.status === "ready" ? state.payload : null;
  const frameReady = usePreviewWindowFrame();

  useEffect(() => {
    if (!frameReady || snapshot === null || restoring.current || externalRestoringRef.current) return;
    // Only the native controller may show the window, guarded by revision.
    let disposed = false;
    void presentPreviewWindow(snapshot.revision).then(() => {
      if (!disposed && !restoring.current && !externalRestoringRef.current && snapshot.takeFocus) document.querySelector<HTMLElement>(".preview-body")?.focus({ preventScroll: true });
    }).catch((error: unknown) => {
      console.error("Could not present preview:", error);
    });
    return () => { disposed = true; };
  }, [externalRestoringRef, frameReady, snapshot]);
  const loadFailure = state.status === "error" ? state.failure : null;
  const failure = renderFailure ?? loadFailure;

  useEffect(() => {
    setRenderFailure(null);
  }, [payload?.entry_id, target]);

  const clearSession = useCallback(() => {
    setSnapshot(null);
    setTarget(null);
    setRenderFailure(null);
  }, []);

  const close = useCallback(() => {
    if (restoring.current || externalRestoringRef.current) return;
    latestRevision.current++;
    clearSession();
    void closePreviewWindow().catch((error: unknown) => {
      console.error("Could not hide the preview window:", error);
    });
  }, [clearSession, externalRestoringRef]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<{ revision: number }>(PREVIEW_CLOSE_EVENT, (event) => {
      if (!disposed && event.payload.revision >= latestRevision.current) {
        latestRevision.current = event.payload.revision;
        clearSession();
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [clearSession]);

  const previousId = payload?.batch?.previous_entry_id ?? null;
  const nextId = payload?.batch?.next_entry_id ?? null;
  const navigate = useCallback((entryId: number) => {
    if (restoring.current || externalRestoringRef.current) return;
    const batchId = payload?.batch?.batch_id ?? target?.batchId ?? null;
    setRenderFailure(null);
    setTarget({ entryId, batchId });
  }, [externalRestoringRef, payload?.batch?.batch_id, target?.batchId]);

  const restoreId = payload?.entry_id ?? null;
  const restore = useCallback(async (keepOpen = true) => {
    if (restoring.current || externalRestoringRef.current || restoreId === null || state.status !== "ready" ||
        payload?.entry_id !== target?.entryId || snapshot === null) return;
    restoring.current = true;
    setLocalRestoring(true);
    restoreGeneration.current++;
    try {
      setRestoreFailed(false);
      setRestoreSucceeded(false);
      await restoreEntry(restoreId);
      setRestoreSucceeded(true);
      if (!keepOpen) {
        const owner = snapshot.owner;
        latestRevision.current++;
        clearSession();
        await closePreviewWindow(owner === "favorites" ? "favorites" : "all");
        await (owner === "favorites" ? closeFavoritesWindow() : closeHistoryWindow());
      }
    } catch (error) {
      setRestoreFailed(true);
      throw error;
    } finally {
      restoring.current = false;
      setLocalRestoring(false);
    }
  }, [clearSession, externalRestoringRef, payload?.entry_id, restoreId, snapshot, state.status, target?.entryId]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || ime.current.blocks(event) || restoring.current || externalRestoringRef.current) return;
      if (isInteractiveTarget(event.target) && event.key !== "Escape") return;
      if (event.key === "Enter" && !event.altKey && !event.metaKey && !event.shiftKey) {
        event.preventDefault();
        void restore(event.ctrlKey).catch((error: unknown) => {
          console.error("Preview restore failed:", error);
          setRestoreFailed(true);
        });
        return;
      }
      if (event.key === "Escape" && !restoring.current) {
        event.preventDefault();
        close();
        return;
      }
      if (event.altKey && event.key === "ArrowLeft" && previousId !== null) {
        event.preventDefault();
        navigate(previousId);
        return;
      }
      if (event.altKey && event.key === "ArrowRight" && nextId !== null) {
        event.preventDefault();
        navigate(nextId);
        return;
      }
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.code === "Space" && !isInteractiveTarget(event.target)) {
        event.preventDefault();
        close();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [close, externalRestoringRef, navigate, nextId, previousId, restore]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void getCurrentWindow().onCloseRequested((event) => {
      event.preventDefault();
      close();
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [close]);

  const appClassName = useMemo(
    () => `app preview-app ${theme}`,
    [theme],
  );

  return (
    <div className={appClassName}
      onCompositionStart={() => ime.current.start()}
      onCompositionEnd={() => ime.current.end()}>
      {restoreSucceeded && <div role="status">{t("history.preview.restored")}</div>}
      {restoreFailed && <div role="alert">{t("history.preview.restoreError")}</div>}
      <PreviewShell
        payload={payload}
        restoringExternally={externalRestoring || localRestoring}
        loading={state.status === "loading" || state.status === "idle"}
        failure={failure}
        onRetry={() => {
          setRenderFailure(null);
          retry();
        }}
        onCorrupt={() => setRenderFailure({
          kind: "corrupt",
          retryable: false,
          sizeBytes: null,
          limitBytes: null,
        })}
        onClose={close}
        onRestore={() => restore()}
        onPrevious={previousId === null ? null : () => navigate(previousId)}
        onNext={nextId === null ? null : () => navigate(nextId)}
        t={t}
      />
    </div>
  );
}
