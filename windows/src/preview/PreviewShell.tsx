import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Check,
  Download,
  FileWarning,
  RefreshCw,
  X,
} from "lucide-react";
import { PreviewContent } from "./PreviewContent";
import type { PreviewFailure } from "./usePreviewPayload";
import type { PreviewPayload } from "../utils/historyPreview";

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) return `${Math.max(0, bytes)} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function failureCopy(failure: PreviewFailure, t: (key: string) => string): string {
  switch (failure.kind) {
    case "too-large":
      return t("history.preview.tooLarge");
    case "corrupt":
      return t("history.preview.corrupt");
    case "unavailable":
      return t("history.preview.unavailable");
    default:
      return t("history.preview.error");
  }
}

export function PreviewShell({
  payload,
  loading,
  failure,
  onRetry,
  onCorrupt,
  onClose,
  onRestore,
  onPrevious,
  onNext,
  t,
}: {
  payload: PreviewPayload | null;
  loading: boolean;
  failure: PreviewFailure | null;
  onRetry: () => void;
  onCorrupt: () => void;
  onClose: () => void;
  onRestore: () => Promise<void>;
  onPrevious: (() => void) | null;
  onNext: (() => void) | null;
  t: (key: string) => string;
}) {
  const [restoreState, setRestoreState] = useState<"idle" | "restoring" | "restored" | "failed">("idle");
  const title = payload?.name ?? t("history.preview.title");
  const position = payload?.batch
    ? `${payload.batch.item_index + 1} / ${payload.batch.item_count}`
    : null;
  const headingId = useMemo(() => `preview-heading-${payload?.entry_id ?? "empty"}`, [payload?.entry_id]);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    setRestoreState("idle");
  }, [payload?.entry_id]);

  useEffect(() => {
    if (restoreState !== "restored") return undefined;
    const timeout = window.setTimeout(() => setRestoreState("idle"), 2400);
    return () => window.clearTimeout(timeout);
  }, [restoreState]);

  const restore = async () => {
    setRestoreState("restoring");
    try {
      await onRestore();
      setRestoreState("restored");
    } catch {
      setRestoreState("failed");
    }
  };

  return (
    <main className="preview-window" role="dialog" aria-modal="false" aria-labelledby={headingId}>
      <header className="preview-titlebar" data-tauri-drag-region>
        <div className="preview-titlebar-copy">
          <FileWarning size={15} aria-hidden="true" />
          <h1 id={headingId}>{title}</h1>
          {payload && <span>{formatBytes(payload.size_bytes)}</span>}
          {position && <span className="preview-position">{position}</span>}
        </div>
        <div className="preview-titlebar-actions" data-tauri-drag-region="false">
          <button type="button" className="preview-window-button" title={t("history.preview.previous")} aria-label={t("history.preview.previous")} disabled={!onPrevious} onClick={onPrevious ?? undefined}>
            <ChevronLeft size={15} aria-hidden="true" />
          </button>
          <button type="button" className="preview-window-button" title={t("history.preview.next")} aria-label={t("history.preview.next")} disabled={!onNext} onClick={onNext ?? undefined}>
            <ChevronRight size={15} aria-hidden="true" />
          </button>
          <button ref={closeRef} type="button" className="preview-window-button is-close" title={t("history.close")} aria-label={t("history.close")} onClick={onClose}>
            <X size={15} aria-hidden="true" />
          </button>
        </div>
      </header>

      <section className="preview-body">
        {loading && <div className="preview-state" role="status" data-testid="preview-loading">{t("history.preview.loading")}</div>}
        {!loading && failure && (
          <div className="preview-state preview-state-error" role="alert" data-testid="preview-error">
            <FileWarning size={28} aria-hidden="true" />
            <h2>{failureCopy(failure, t)}</h2>
            {failure.kind === "too-large" && failure.limitBytes && <p>{t("history.preview.limit").replace("{size}", formatBytes(failure.limitBytes))}</p>}
            {failure.retryable && <button type="button" className="preview-primary-button" onClick={onRetry}><RefreshCw size={14} aria-hidden="true" />{t("history.preview.retry")}</button>}
          </div>
        )}
        {!loading && !failure && payload && <PreviewContent payload={payload} t={t} onCorrupt={onCorrupt} />}
        {!loading && !failure && !payload && <div className="preview-state">{t("history.preview.loading")}</div>}
      </section>

      <footer className="preview-footer">
        <span className="preview-footer-meta">
          {payload?.batch ? `${t("history.preview.batch")} ${position}` : ""}
          {restoreState === "restored" && (
            <span className="preview-footer-success" role="status">
              <Check size={14} aria-hidden="true" />
              {t("history.preview.restored")}
            </span>
          )}
          {restoreState === "failed" && <span className="preview-footer-error" role="alert">{t("history.preview.restoreError")}</span>}
        </span>
        <div className="preview-footer-actions">
          {payload && (
            <button type="button" className="preview-secondary-button" disabled={restoreState === "restoring"} onClick={() => void restore()}>
              <Download size={14} aria-hidden="true" />
              {restoreState === "restoring" ? t("history.preview.restoring") : t("history.restoreEntry")}
            </button>
          )}
        </div>
      </footer>
    </main>
  );
}
