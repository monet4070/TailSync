import { useEffect, useLayoutEffect, useRef } from "react";
import { ackSyncWarning, type SyncWarning } from "../tailsyncClient";
import type { HistoryNotice, HistoryNoticeInput, HistoryNoticeResult } from "./useHistoryNotice";

/** Consume a daemon warning only after its inline notice has committed visibly. */
export function useSyncWarningNotice(
  warning: SyncWarning | null,
  message: string | null,
  notice: HistoryNotice | null,
  show: (input: HistoryNoticeInput) => HistoryNoticeResult,
): void {
  const receipt = useRef({ id: 0, displayed: false, acknowledged: false, inFlight: false });
  const renderedKey = useRef(notice?.key);
  useLayoutEffect(() => { renderedKey.current = notice?.key; }, [notice?.key]);
  const id = warning?.id;

  useEffect(() => {
    if (id === undefined || message === null) return;
    if (receipt.current.id !== id) {
      receipt.current = { id, displayed: false, acknowledged: false, inFlight: false };
    }
    const current = receipt.current;
    const key = `sync-warning:${id}`;
    let disposed = false;
    let retryTimer: number | undefined;
    const retry = (ms: number) => {
      if (disposed) return;
      window.clearTimeout(retryTimer);
      retryTimer = window.setTimeout(attempt, Math.max(1, ms));
    };
    const attempt = () => {
      if (disposed || document.visibilityState !== "visible" || current.acknowledged) return;
      if (renderedKey.current === key) current.displayed = true;
      if (current.displayed) {
        // The same snapshot can recur after an RPC error. Retry its ack without
        // incrementing occurrences or extending the notice's bounded lifetime.
        if (current.inFlight) { retry(1000); return; }
        current.inFlight = true;
        void ackSyncWarning(id).then(() => {
          // false also means that this id has already been replaced/consumed.
          current.acknowledged = true;
        }).catch(() => retry(1000)).finally(() => { current.inFlight = false; });
        return;
      }
      const result = show({ key, level: "warning", message });
      if (!result.accepted) retry(result.retryAfterMs);
      // Accepted is not a display receipt. The effect runs again after the
      // rendered notice key changes, and only that committed key permits ack.
    };
    attempt();
    document.addEventListener("visibilitychange", attempt);
    return () => {
      disposed = true;
      window.clearTimeout(retryTimer);
      document.removeEventListener("visibilitychange", attempt);
    };
  }, [id, message, notice?.key, show]);
}
