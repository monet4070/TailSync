import { describe, expect, it } from "vitest";
import { rowPopoverPosition } from "./historyRowPopoverPosition";

describe("row popover positioning in a compact window", () => {
  const viewport = { width: 400, height: 600 };
  const size = { width: 268, height: 244 };

  it("aligns with the button and opens below when it fits", () => {
    const result = rowPopoverPosition({ left: 348, right: 372, top: 100, bottom: 122 }, size, viewport);
    expect(result).toMatchObject({ side: "bottom", left: 104, top: 128 });
  });

  it("flips above the last visible row without overflowing the native window", () => {
    const result = rowPopoverPosition({ left: 348, right: 372, top: 560, bottom: 582 }, size, viewport);
    expect(result).toMatchObject({ side: "top", top: 310 });
    expect(result.top + size.height).toBeLessThan(600);
  });

  it("clamps a wide panel at the left edge", () => {
    const result = rowPopoverPosition({ left: 18, right: 42, top: 100, bottom: 122 }, size, viewport);
    expect(result.left).toBe(8);
  });

  it("allows a tall details panel to scroll in the larger available side", () => {
    const result = rowPopoverPosition({ left: 200, right: 224, top: 200, bottom: 222 },
      { width: 268, height: 900 }, { width: 320, height: 400 });
    expect(result).toMatchObject({ side: "top", top: 8, maxHeight: 186 });
  });
});
