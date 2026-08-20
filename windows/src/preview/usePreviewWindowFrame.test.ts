import { describe, expect, it } from "vitest";
import {
  clampPreviewWindowFrame,
  parsePreviewWindowFrame,
} from "./usePreviewWindowFrame";

describe("preview window frames", () => {
  it("rejects malformed persisted values", () => {
    expect(parsePreviewWindowFrame(null)).toBeNull();
    expect(parsePreviewWindowFrame("not-json")).toBeNull();
    expect(parsePreviewWindowFrame('{"x":0,"y":0,"width":0,"height":500}')).toBeNull();
  });

  it("clamps an off-screen frame to the nearest usable work area", () => {
    expect(clampPreviewWindowFrame(
      { x: 5_000, y: 5_000, width: 900, height: 700 },
      [{ x: 100, y: 50, width: 1_600, height: 900 }],
    )).toEqual({ x: 800, y: 250, width: 900, height: 700 });
  });

  it("applies renderer minimums without exceeding a small monitor", () => {
    expect(clampPreviewWindowFrame(
      { x: 0, y: 0, width: 200, height: 100 },
      [{ x: 0, y: 0, width: 700, height: 500 }],
    )).toEqual({ x: 0, y: 0, width: 640, height: 420 });
  });
});
