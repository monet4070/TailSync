import { X } from "lucide-react";
import type { HistoryNotice } from "../../hooks/useHistoryNotice";
import type { Translate } from "./HistoryViewTypes";

interface HistoryNoticeBarProps {
  t: Translate;
  notice: HistoryNotice | null;
  onDismiss: () => void;
}

export function HistoryNoticeBar({ t, notice, onDismiss }: HistoryNoticeBarProps) {
  if (!notice) return null;
  return (
    <div
      className="history-notice"
      data-level={notice.level}
      role={notice.level === "error" ? "alert" : "status"}
    >
      <span className="history-notice-message">{notice.message}</span>
      {notice.occurrences > 1 && (
        <span className="history-notice-count" aria-label={`${notice.occurrences} occurrences`}>
          ×{notice.occurrences}
        </span>
      )}
      <button
        className="history-notice-dismiss"
        type="button"
        aria-label={t("history.dismissNotice")}
        title={t("history.dismissNotice")}
        onClick={onDismiss}
      >
        <X size={14} strokeWidth={1.8} aria-hidden="true" />
      </button>
    </div>
  );
}
