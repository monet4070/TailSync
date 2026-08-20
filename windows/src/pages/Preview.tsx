import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "../hooks/useI18n";
import { useTheme } from "../hooks/useTheme";
import {
  closePreviewWindow,
  getPreviewWindowRequest,
  restoreEntry,
  type PreviewWindowSnapshot,
} from "../tailsyncClient";
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
    (snapshot.batchId === null || typeof snapshot.batchId === "string");
}

export function Preview() {
  const { theme } = useTheme();
  const { t } = useI18n();
  const [target, setTarget] = useState<PreviewTarget | null>(null);
  const [renderFailure, setRenderFailure] = useState<PreviewFailure | null>(null);
  const latestRevision = useRef(0);
  const { state, retry } = usePreviewPayload(target);

  const applySnapshot = useCallback((snapshot: PreviewWindowSnapshot | null) => {
    if (!validSnapshot(snapshot) || snapshot.revision <= latestRevision.current) return;
    latestRevision.current = snapshot.revision;
    setRenderFailure(null);
    setTarget({ entryId: snapshot.entryId, batchId: snapshot.batchId });
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<PreviewWindowSnapshot>(PREVIEW_REQUEST_EVENT, (event) => {
      if (!disposed) applySnapshot(event.payload);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    void getPreviewWindowRequest().then((snapshot) => {
      if (!disposed) applySnapshot(snapshot);
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
    if (!frameReady || target === null) return undefined;
    let disposed = false;
    const appWindow = getCurrentWindow();
    void appWindow.isVisible()
      .then(async (visible) => {
        if (disposed) return;
        if (!visible) await appWindow.show();
        if (!disposed) await appWindow.setFocus();
      })
      .catch((error: unknown) => {
        if (!disposed) console.error("Could not show the preview window:", error);
      });
    return () => {
      disposed = true;
    };
  }, [frameReady, target]);
  const loadFailure = state.status === "error" ? state.failure : null;
  const failure = renderFailure ?? loadFailure;

  useEffect(() => {
    setRenderFailure(null);
  }, [payload?.entry_id, target]);

  const clearSession = useCallback(() => {
    setTarget(null);
    setRenderFailure(null);
  }, []);

  const close = useCallback(() => {
    clearSession();
    void closePreviewWindow().catch((error: unknown) => {
      console.error("Could not hide the preview window:", error);
    });
  }, [clearSession]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen(PREVIEW_CLOSE_EVENT, () => {
      if (!disposed) clearSession();
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
    const batchId = payload?.batch?.batch_id ?? target?.batchId ?? null;
    setRenderFailure(null);
    setTarget({ entryId, batchId });
  }, [payload?.batch?.batch_id, target?.batchId]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat) return;
      if (event.key === "Escape") {
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
  }, [close, navigate, nextId, previousId]);

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

  const restoreId = payload?.entry_id ?? target?.entryId ?? null;
  const restore = useCallback(async () => {
    if (restoreId === null) return;
    await restoreEntry(restoreId);
  }, [restoreId]);

  const appClassName = useMemo(
    () => `app preview-app ${theme}`,
    [theme],
  );

  return (
    <div className={appClassName}>
      <PreviewShell
        payload={payload}
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
        onRestore={restore}
        onPrevious={previousId === null ? null : () => navigate(previousId)}
        onNext={nextId === null ? null : () => navigate(nextId)}
        t={t}
      />
    </div>
  );
}
