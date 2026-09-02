import {
  ArrowLeft,
  ArrowRight,
  Square,
  Trash2,
} from "lucide-react";
import type { HistoryFooterProps } from "./HistoryViewTypes";
import { formatSize } from "./HistoryEntryHelpers";
import { HistoryNoticeBar } from "./HistoryNoticeBar";

export function HistoryFooter({
  t,
  entriesLength,
  hasPrev,
  hasNext,
  page,
  totalEntries,
  totalPages,
  setPage,
  progressBarEnabled,
  fileProgress,
  handleCancelFileBatch,
  showClearConfirm,
  clearing,
  setShowClearConfirm,
  handleClearHistory,
  isFavoritesCollection,
  historyNotice,
  clearHistoryNotice,
  retryHistory,
}: HistoryFooterProps) {
  return (
    <>
      {entriesLength > 0 && (
        <div className="pagination">
          <button
            className="page-btn"
            disabled={!hasPrev}
            onClick={() => {
              setPage((current) => current - 1);
              document.querySelector(".history-list")?.scrollTo({ top: 0 });
            }}
          >
            <ArrowLeft size={14} strokeWidth={1.8} aria-hidden="true" />
            {t("history.prev")}
          </button>
          <span className="page-info">
            {totalEntries === null ? page + 1 : `${page + 1} / ${totalPages}`}
          </span>
          <button
            className="page-btn"
            disabled={!hasNext}
            onClick={() => {
              setPage((current) => current + 1);
              document.querySelector(".history-list")?.scrollTo({ top: 0 });
            }}
          >
            {t("history.next")}
            <ArrowRight size={14} strokeWidth={1.8} aria-hidden="true" />
          </button>
        </div>
      )}

      {progressBarEnabled && fileProgress?.active && (
        <div className="file-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${Math.round((fileProgress.sent / Math.max(fileProgress.total, 1)) * 100)}%`,
              }}
            />
          </div>
          <span className="progress-text">
            <strong>{Math.min(fileProgress.completed_files + 1, fileProgress.total_files)}/{fileProgress.total_files}</strong>
            <span title={fileProgress.name}>{fileProgress.name}</span>
            {fileProgress.device && <span>{fileProgress.device}</span>}
            <span>{formatSize(fileProgress.sent)} / {formatSize(fileProgress.total)}</span>
            <span>{formatSize(fileProgress.speed_bytes_per_second)}/s</span>
          </span>
          {fileProgress.can_stop && fileProgress.batch_id && (
            <button
              className="progress-stop"
              type="button"
              title={t("history.stopTransfer")}
              onClick={() => void handleCancelFileBatch(fileProgress.batch_id)}
            >
              <Square size={12} fill="currentColor" aria-hidden="true" />
              <span>{t("history.stopTransfer")}</span>
            </button>
          )}
        </div>
      )}

      <HistoryNoticeBar
        t={t}
        notice={historyNotice}
        onDismiss={clearHistoryNotice}
        action={historyNotice?.key === "history-load"
          ? { label: t("history.retry"), onClick: retryHistory }
          : undefined}
      />

      {!isFavoritesCollection && showClearConfirm && (
        <div className="dialog-backdrop" onMouseDown={() => !clearing && setShowClearConfirm(false)}>
          <div
            className="confirm-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="clear-history-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="confirm-dialog-icon">
              <Trash2 size={22} strokeWidth={1.6} aria-hidden="true" />
            </div>
            <h2 id="clear-history-title">{t("history.clearConfirmTitle")}</h2>
            <p>{t("history.clearConfirmDescription")}</p>
            <div className="confirm-dialog-actions">
              <button type="button" onClick={() => setShowClearConfirm(false)} disabled={clearing}>
                {t("history.cancel")}
              </button>
              <button type="button" className="danger" onClick={() => void handleClearHistory()} disabled={clearing}>
                {t(clearing ? "history.clearing" : "history.clearAll")}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
