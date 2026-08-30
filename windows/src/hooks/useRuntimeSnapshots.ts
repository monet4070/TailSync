import { useEffect, useRef } from "react";
import {
  waitRuntimeSnapshot,
  type RuntimeSnapshot,
} from "../tailsyncClient";

const RETRY_DELAY_MS = 1_000;

function validSnapshot(value: RuntimeSnapshot | null | undefined): value is RuntimeSnapshot {
  return Boolean(
    value &&
    Number.isSafeInteger(value.revision) && value.revision > 0 &&
    Number.isSafeInteger(value.history_version) && value.history_version >= 0,
  );
}

export function useRuntimeSnapshots(
  onSnapshot: (snapshot: RuntimeSnapshot) => void | Promise<void>,
  waitMs: number,
): void {
  const callbackRef = useRef(onSnapshot);
  callbackRef.current = onSnapshot;

  useEffect(() => {
    let disposed = false;
    let revision = 0;
    let notificationId = 0;
    let retryTimer = 0;
    let finishRetry: (() => void) | undefined;

    const retryDelay = () => new Promise<void>((resolve) => {
      const finish = () => {
        finishRetry = undefined;
        resolve();
      };
      finishRetry = finish;
      retryTimer = window.setTimeout(finish, RETRY_DELAY_MS);
    });
    const run = async () => {
      while (!disposed) {
        try {
          const snapshot = await waitRuntimeSnapshot(revision, waitMs, notificationId);
          if (disposed) return;
          if (!validSnapshot(snapshot)) throw new Error("Invalid runtime snapshot");
          revision = snapshot.revision;
          notificationId = snapshot.notifications.reduce(
            (latest, notification) => Math.max(latest, notification.id),
            notificationId,
          );
          await callbackRef.current(snapshot);
        } catch (error) {
          if (disposed) return;
          console.error("Runtime snapshot wait failed:", error);
          await retryDelay();
        }
      }
    };

    void run();
    return () => {
      disposed = true;
      window.clearTimeout(retryTimer);
      finishRetry?.();
    };
  }, [waitMs]);
}
