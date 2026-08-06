import { useEffect, useRef } from "react";

export function useVisiblePolling(
  task: () => void | Promise<void>,
  intervalMs: number,
): void {
  const taskRef = useRef(task);
  taskRef.current = task;

  useEffect(() => {
    let active = true;
    let busy = false;
    let timer = 0;

    const schedule = () => {
      window.clearTimeout(timer);
      if (active && document.visibilityState === "visible") {
        timer = window.setTimeout(() => void run(), intervalMs);
      }
    };
    const run = async () => {
      if (!active || busy || document.visibilityState !== "visible") return;
      busy = true;
      try {
        await taskRef.current();
      } finally {
        busy = false;
        schedule();
      }
    };
    const handleVisibilityChange = () => {
      window.clearTimeout(timer);
      if (document.visibilityState === "visible") void run();
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    void run();
    return () => {
      active = false;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [intervalMs]);
}
