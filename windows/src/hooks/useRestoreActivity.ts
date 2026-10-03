import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type RestoreActivity = { active: boolean; epoch: number; previewRevision?: number };

/** Backend is the authoritative exclusion gate; this event also freezes UI work. */
export function useRestoreActivity(onBegin: (activity: RestoreActivity) => void) {
  const activeRef = useRef(false);
  const [active, setActive] = useState(false);
  const callback = useRef(onBegin);
  useLayoutEffect(() => { callback.current = onBegin; }, [onBegin]);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let epoch = -1;
    let ended = false;
    void listen<RestoreActivity>("tailsync://restore-state", ({ payload }) => {
      if (disposed || payload.epoch < epoch || (payload.epoch === epoch && ended)) return;
      if (payload.epoch > epoch) ended = false;
      epoch = payload.epoch;
      ended = !payload.active;
      activeRef.current = payload.active;
      setActive(payload.active);
      if (payload.active) callback.current(payload);
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; }).catch(console.error);
    return () => { disposed = true; stop?.(); };
  }, []);
  return { active, activeRef };
}
