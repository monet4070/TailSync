import { useCallback, useEffect, useRef, useState } from "react";

export type HistoryNoticeLevel = "success" | "warning" | "error";

export interface HistoryNotice {
  key: string;
  level: HistoryNoticeLevel;
  message: string;
  occurrences: number;
}

export interface HistoryNoticeInput {
  key: string;
  level: HistoryNoticeLevel;
  message: string;
}

const NOTICE_TTL_MS: Record<HistoryNoticeLevel, number> = {
  success: 1_500,
  warning: 4_500,
  error: 6_000,
};
const NOTICE_VISIBLE_BUDGET_MS = 8_000;
const NOTICE_COOLDOWN_MS = 1_500;

/**
 * A bounded, inline history notice. Repeated copies of the same semantic
 * warning update its count but keep the original deadline, so a noisy worker
 * cannot keep a notice alive forever.
 */
export function useHistoryNotice() {
  const [notice, setNotice] = useState<HistoryNotice | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const active = useRef<{
    key: string;
    expiresAt: number;
    visibleUntil: number;
  } | null>(null);
  const mutedUntil = useRef(0);

  useEffect(() => () => {
    if (timer.current !== undefined) window.clearTimeout(timer.current);
  }, []);

  const clear = useCallback(() => {
    if (timer.current !== undefined) window.clearTimeout(timer.current);
    timer.current = undefined;
    active.current = null;
    setNotice(null);
  }, []);

  const show = useCallback((input: HistoryNoticeInput) => {
    const now = Date.now();
    if (now < mutedUntil.current) return;
    const current = active.current;
    if (current && current.visibleUntil <= now) {
      if (timer.current !== undefined) window.clearTimeout(timer.current);
      timer.current = undefined;
      active.current = null;
      mutedUntil.current = now + NOTICE_COOLDOWN_MS;
      setNotice(null);
      return;
    }
    if (current && current.key === input.key && current.expiresAt > now) {
      setNotice((previous) => previous && previous.key === input.key
        ? { ...previous, message: input.message, occurrences: previous.occurrences + 1 }
        : previous);
      return;
    }

    if (timer.current !== undefined) window.clearTimeout(timer.current);
    const visibleUntil = current?.visibleUntil ?? now + NOTICE_VISIBLE_BUDGET_MS;
    const expiresAt = Math.min(now + NOTICE_TTL_MS[input.level], visibleUntil);
    active.current = { key: input.key, expiresAt, visibleUntil };
    setNotice({ ...input, occurrences: 1 });
    timer.current = window.setTimeout(() => {
      const latest = active.current;
      if (latest?.key !== input.key || latest.expiresAt !== expiresAt) return;
      if (expiresAt >= latest.visibleUntil) {
        mutedUntil.current = Date.now() + NOTICE_COOLDOWN_MS;
      }
      active.current = null;
      timer.current = undefined;
      setNotice(null);
    }, Math.max(0, expiresAt - now));
  }, []);

  return [notice, show, clear] as const;
}
