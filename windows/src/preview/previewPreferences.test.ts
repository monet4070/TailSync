import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_PREVIEW_TEXT_FONT_SIZE,
  PREVIEW_TEXT_FONT_SIZE_KEY,
  clampPreviewTextFontSize,
  isModifierZoomGesture,
  usePreviewTextFontSize,
  zoomFromWheel,
} from "./previewPreferences";

describe("preview preferences", () => {
  beforeEach(() => localStorage.clear());

  it("defaults to readable 18px text and remembers a valid change", () => {
    const { result } = renderHook(() => usePreviewTextFontSize());
    expect(result.current[0]).toBe(DEFAULT_PREVIEW_TEXT_FONT_SIZE);

    act(() => result.current[1](22));

    expect(result.current[0]).toBe(22);
    expect(localStorage.getItem(PREVIEW_TEXT_FONT_SIZE_KEY)).toBe("22");
  });

  it("clamps malformed or out-of-range stored sizes", () => {
    expect(clampPreviewTextFontSize(Number.NaN)).toBe(18);
    expect(clampPreviewTextFontSize(2)).toBe(12);
    expect(clampPreviewTextFontSize(99)).toBe(32);

    localStorage.setItem(PREVIEW_TEXT_FONT_SIZE_KEY, "99");
    const { result } = renderHook(() => usePreviewTextFontSize());
    expect(result.current[0]).toBe(32);
  });

  it("zooms only for Control or Command wheel gestures", () => {
    expect(isModifierZoomGesture({ ctrlKey: false, metaKey: false })).toBe(false);
    expect(isModifierZoomGesture({ ctrlKey: true, metaKey: false })).toBe(true);
    expect(isModifierZoomGesture({ ctrlKey: false, metaKey: true })).toBe(true);
    expect(zoomFromWheel(1, -100, 0.5, 3)).toBe(1.1);
    expect(zoomFromWheel(1, 100, 0.5, 3)).toBe(0.909);
    expect(zoomFromWheel(3, -100, 0.5, 3)).toBe(3);
  });
});
