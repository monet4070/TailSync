import { useEffect, useRef, useState, type RefObject } from "react";

export const PREVIEW_TEXT_FONT_SIZE_KEY = "tailsync-preview-text-font-size";
export const DEFAULT_PREVIEW_TEXT_FONT_SIZE = 18;
export const MIN_PREVIEW_TEXT_FONT_SIZE = 12;
export const MAX_PREVIEW_TEXT_FONT_SIZE = 32;

export function clampPreviewTextFontSize(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_PREVIEW_TEXT_FONT_SIZE;
  return Math.min(MAX_PREVIEW_TEXT_FONT_SIZE, Math.max(MIN_PREVIEW_TEXT_FONT_SIZE, Math.round(value)));
}

function readPreviewTextFontSize(): number {
  try {
    const stored = window.localStorage.getItem(PREVIEW_TEXT_FONT_SIZE_KEY);
    return stored === null
      ? DEFAULT_PREVIEW_TEXT_FONT_SIZE
      : clampPreviewTextFontSize(Number(stored));
  } catch {
    return DEFAULT_PREVIEW_TEXT_FONT_SIZE;
  }
}

export function usePreviewTextFontSize(): [number, (size: number | ((current: number) => number)) => void] {
  const [fontSize, setFontSize] = useState(readPreviewTextFontSize);

  useEffect(() => {
    try {
      window.localStorage.setItem(PREVIEW_TEXT_FONT_SIZE_KEY, String(fontSize));
    } catch {
      // A disabled storage backend should not make preview unavailable.
    }
  }, [fontSize]);

  const updateFontSize = (size: number | ((current: number) => number)) => {
    setFontSize((current) => clampPreviewTextFontSize(
      typeof size === "function" ? size(current) : size,
    ));
  };

  return [fontSize, updateFontSize];
}

export function isModifierZoomGesture(event: Pick<WheelEvent, "ctrlKey" | "metaKey">): boolean {
  return event.ctrlKey || event.metaKey;
}

export function zoomFromWheel(
  current: number,
  deltaY: number,
  minimum: number,
  maximum: number,
): number {
  if (deltaY === 0) return current;
  const factor = deltaY < 0 ? 1.1 : 1 / 1.1;
  return Math.min(maximum, Math.max(minimum, Number((current * factor).toFixed(3))));
}

export function useModifierWheelZoom<T extends HTMLElement>(
  onZoom: (deltaY: number) => void,
): RefObject<T | null> {
  const elementRef = useRef<T>(null);
  const callbackRef = useRef(onZoom);
  callbackRef.current = onZoom;

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return undefined;
    const handleWheel = (event: WheelEvent) => {
      if (!isModifierZoomGesture(event)) return;
      event.preventDefault();
      callbackRef.current(event.deltaY);
    };
    element.addEventListener("wheel", handleWheel, { passive: false });
    return () => element.removeEventListener("wheel", handleWheel);
  }, []);

  return elementRef;
}
