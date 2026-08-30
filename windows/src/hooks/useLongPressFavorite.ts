import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

/** The grace period keeps an ordinary click from feeling delayed. */
export const LONG_PRESS_GRACE_MS = 220;
/** The visible charge follows the 0.42 s motion specified by the feature. */
export const LONG_PRESS_CHARGE_MS = 420;
/** Match macOS: keep the completion stamp briefly before an unfavorite fades. */
export const FAVORITE_STAMP_VISIBLE_MS = 550;
const MOVE_CANCEL_DISTANCE_PX = 8;

interface ActivePointer {
  id: number;
  x: number;
  y: number;
  action: FavoriteTriggerAction;
}

export type FavoriteTriggerAction = "favorite" | "unfavorite";

export interface LongPressFavoriteResult {
  progress: number;
  isCharging: boolean;
  isTriggered: boolean;
  triggeredAction: FavoriteTriggerAction | null;
  suppressClick: () => boolean;
  suppressContextMenu: () => boolean;
  cancel: () => void;
  onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => void;
}

/**
 * Pointer-based long press controller for history rows.
 *
 * It deliberately uses two one-shot timers (grace + commit), not a per-frame
 * polling loop. CSS owns the progress interpolation, while the returned
 * `isTriggered` flag lets the row show its completion stamp without stealing
 * the normal click/double-click path.
 */
export function useLongPressFavorite(
  onComplete: () => void,
  enabled = true,
  isFavorite = false,
): LongPressFavoriteResult {
  const activePointer = useRef<ActivePointer | null>(null);
  const graceTimer = useRef<number | null>(null);
  const commitTimer = useRef<number | null>(null);
  const stampTimer = useRef<number | null>(null);
  const triggeredRef = useRef(false);
  const [progress, setProgress] = useState(0);
  const [isCharging, setIsCharging] = useState(false);
  const [triggeredAction, setTriggeredAction] = useState<FavoriteTriggerAction | null>(null);

  const clearTimers = useCallback(() => {
    if (graceTimer.current !== null) {
      window.clearTimeout(graceTimer.current);
      graceTimer.current = null;
    }
    if (commitTimer.current !== null) {
      window.clearTimeout(commitTimer.current);
      commitTimer.current = null;
    }
  }, []);

  const clearStampTimer = useCallback(() => {
    if (stampTimer.current !== null) {
      window.clearTimeout(stampTimer.current);
      stampTimer.current = null;
    }
  }, []);

  const cancel = useCallback(() => {
    clearTimers();
    activePointer.current = null;
    setIsCharging(false);
    setProgress(0);
  }, [clearTimers]);

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (!enabled || event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, a, [role='button']")) return;

    clearTimers();
    clearStampTimer();
    triggeredRef.current = false;
    setTriggeredAction(null);
    setIsCharging(false);
    setProgress(0);
    activePointer.current = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      action: isFavorite ? "unfavorite" : "favorite",
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);

    graceTimer.current = window.setTimeout(() => {
      const pointer = activePointer.current;
      if (!pointer || pointer.id !== event.pointerId) return;
      setIsCharging(true);
      setProgress(1);
      commitTimer.current = window.setTimeout(() => {
        const current = activePointer.current;
        if (!current || current.id !== event.pointerId) return;
        triggeredRef.current = true;
        setIsCharging(false);
        setTriggeredAction(current.action);
        onComplete();
        stampTimer.current = window.setTimeout(() => {
          stampTimer.current = null;
          setTriggeredAction(null);
        }, FAVORITE_STAMP_VISIBLE_MS);
      }, LONG_PRESS_CHARGE_MS);
    }, LONG_PRESS_GRACE_MS);
  }, [clearStampTimer, clearTimers, enabled, isFavorite, onComplete]);

  const onPointerMove = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    const pointer = activePointer.current;
    if (!pointer || pointer.id !== event.pointerId) return;
    const distance = Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y);
    if (distance > MOVE_CANCEL_DISTANCE_PX) cancel();
  }, [cancel]);

  const onPointerUp = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (activePointer.current?.id !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    cancel();
  }, [cancel]);

  const onPointerCancel = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (activePointer.current?.id !== event.pointerId) return;
    cancel();
  }, [cancel]);

  const suppressClick = useCallback(() => triggeredRef.current, []);
  const suppressContextMenu = useCallback(
    () => activePointer.current !== null && triggeredRef.current,
    [],
  );

  useEffect(() => () => {
    clearTimers();
    clearStampTimer();
  }, [clearStampTimer, clearTimers]);

  return {
    progress,
    isCharging,
    isTriggered: triggeredAction !== null,
    triggeredAction,
    suppressClick,
    suppressContextMenu,
    cancel,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  };
}
