// Transient state with automatic revert (T251 extraction from History.tsx).
//
// `flash(value)` sets the state and schedules a revert to the initial value
// after `durationMs`, restarting the timer on repeated flashes; `clear()`
// reverts immediately and cancels any pending timer. Used for toasts and
// restore feedback that should disappear on their own.

import { useCallback, useEffect, useRef, useState } from "react";

export function useTransient<T>(initial: T, durationMs: number) {
  const [value, setValue] = useState<T>(initial);
  const timer = useRef(0);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const flash = useCallback(
    (next: T) => {
      setValue(next);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setValue(initial), durationMs);
    },
    [initial, durationMs],
  );

  const clear = useCallback(() => {
    window.clearTimeout(timer.current);
    setValue(initial);
  }, [initial]);

  return [value, flash, clear] as const;
}
