import { useEffect, useRef, useState } from "react";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";

export interface PreviewWindowFrame {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface WorkArea extends PreviewWindowFrame {}

export const PREVIEW_WINDOW_FRAME_KEY = "tailsync.preview.frame.v2";
const LEGACY_FRAME_KEYS = [
  "tailsync.preview.frame.v1.text",
  "tailsync.preview.frame.v1.document",
  "tailsync.preview.frame.v1.image",
  "tailsync.preview.frame.v1.pdf",
];
const SAVE_DELAY_MS = 180;
const MINIMUM_SIZE = { width: 640, height: 420 };

function finiteInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value);
}

export function parsePreviewWindowFrame(value: string | null): PreviewWindowFrame | null {
  if (value === null) return null;
  try {
    const parsed = JSON.parse(value) as Partial<PreviewWindowFrame>;
    if (
      !finiteInteger(parsed.x) ||
      !finiteInteger(parsed.y) ||
      !finiteInteger(parsed.width) ||
      !finiteInteger(parsed.height) ||
      parsed.width <= 0 ||
      parsed.height <= 0
    ) {
      return null;
    }
    return {
      x: parsed.x,
      y: parsed.y,
      width: parsed.width,
      height: parsed.height,
    };
  } catch {
    return null;
  }
}

function distanceToArea(frame: PreviewWindowFrame, area: WorkArea): number {
  const frameX = frame.x + frame.width / 2;
  const frameY = frame.y + frame.height / 2;
  const areaX = Math.max(area.x, Math.min(frameX, area.x + area.width));
  const areaY = Math.max(area.y, Math.min(frameY, area.y + area.height));
  return (frameX - areaX) ** 2 + (frameY - areaY) ** 2;
}

export function clampPreviewWindowFrame(
  frame: PreviewWindowFrame,
  workAreas: readonly WorkArea[],
): PreviewWindowFrame | null {
  if (workAreas.length === 0) return null;
  const area = workAreas.reduce((closest, candidate) =>
    distanceToArea(frame, candidate) < distanceToArea(frame, closest) ? candidate : closest,
  );
  const width = Math.min(area.width, Math.max(MINIMUM_SIZE.width, frame.width));
  const height = Math.min(area.height, Math.max(MINIMUM_SIZE.height, frame.height));
  return {
    x: Math.max(area.x, Math.min(frame.x, area.x + area.width - width)),
    y: Math.max(area.y, Math.min(frame.y, area.y + area.height - height)),
    width,
    height,
  };
}

function readStoredFrame(): PreviewWindowFrame | null {
  const keys = [PREVIEW_WINDOW_FRAME_KEY, ...LEGACY_FRAME_KEYS];
  for (const key of keys) {
    const frame = parsePreviewWindowFrame(localStorage.getItem(key));
    if (frame !== null) return frame;
  }
  return null;
}

/** Persist and restore one frame for the reusable preview window. */
export function usePreviewWindowFrame(): boolean {
  const [ready, setReady] = useState(false);
  const restoring = useRef(false);

  useEffect(() => {
    let disposed = false;
    let saveTimer: number | null = null;
    const unlisteners: Array<() => void> = [];
    const appWindow = getCurrentWindow();

    const save = () => {
      if (restoring.current) return;
      if (saveTimer !== null) window.clearTimeout(saveTimer);
      saveTimer = window.setTimeout(() => {
        saveTimer = null;
        if (disposed || restoring.current) return;
        void Promise.all([appWindow.outerPosition(), appWindow.outerSize()])
          .then(([position, size]) => {
            if (disposed || restoring.current) return;
            const frame: PreviewWindowFrame = {
              x: position.x,
              y: position.y,
              width: size.width,
              height: size.height,
            };
            localStorage.setItem(PREVIEW_WINDOW_FRAME_KEY, JSON.stringify(frame));
          })
          .catch((error: unknown) => {
            console.error("Could not save preview-window frame:", error);
          });
      }, SAVE_DELAY_MS);
    };

    void appWindow.onMoved(save).then((unlisten) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    });
    void appWindow.onResized(save).then((unlisten) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    });
    return () => {
      disposed = true;
      if (saveTimer !== null) window.clearTimeout(saveTimer);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let stored: PreviewWindowFrame | null = null;
    try {
      stored = readStoredFrame();
    } catch (error) {
      console.error("Could not read the preview-window frame:", error);
    }
    if (stored === null) {
      setReady(true);
      return () => {
        disposed = true;
      };
    }

    restoring.current = true;
    const appWindow = getCurrentWindow();
    void availableMonitors()
      .then((monitors) => clampPreviewWindowFrame(stored!, monitors.map(({ workArea }) => ({
        x: workArea.position.x,
        y: workArea.position.y,
        width: workArea.size.width,
        height: workArea.size.height,
      }))))
      .then(async (frame) => {
        if (disposed) return;
        if (frame === null) {
          await appWindow.center();
          return;
        }
        await appWindow.setSize(new PhysicalSize(frame.width, frame.height));
        await appWindow.setPosition(new PhysicalPosition(frame.x, frame.y));
        localStorage.setItem(PREVIEW_WINDOW_FRAME_KEY, JSON.stringify(frame));
      })
      .catch((error: unknown) => {
        console.error("Could not restore preview-window frame:", error);
      })
      .finally(() => {
        restoring.current = false;
        if (!disposed) setReady(true);
      });
    return () => {
      disposed = true;
    };
  }, []);

  return ready;
}
