import { describe, expect, it } from "vitest";
import { HistoryImeGuard, visibleHistoryIds } from "./historyKeyboard";
import type { HistoryEntry } from "../tailsyncClient";

describe("history keyboard helpers", () => {
  it("protects composition, keyCode229, and exactly the post-composition grace window", () => {
    const ime = new HistoryImeGuard();
    const normal = { isComposing: false, keyCode: 13 };
    expect(ime.blocks({ ...normal, isComposing: true }, 0)).toBe(true);
    expect(ime.blocks({ ...normal, keyCode: 229 }, 0)).toBe(true);
    ime.start();
    expect(ime.blocks(normal, 500)).toBe(true);
    ime.end(500);
    expect(ime.blocks(normal, 599)).toBe(true);
    expect(ime.blocks(normal, 600)).toBe(false);
  });

  it("returns visual group order and excludes collapsed batch overflow", () => {
    const now = new Date(2026, 7, 15, 12);
    const make = (id: number, daysAgo: number, batch: string | null = null): HistoryEntry => ({
      id, timestamp: new Date(2026, 7, 15 - daysAgo, 9).toISOString(), type: batch ? "file" : "text",
      description: String(id), data_hash: String(id), size_bytes: 1, source_peer: "peer",
      batch_id: batch, batch_total: batch ? 3 : null,
    });
    const entries = [make(4, 2), make(1, 0, "batch"), make(2, 0, "batch"), make(3, 0, "batch")];
    expect(visibleHistoryIds(entries, new Set(), now)).toEqual([1, 2, 4]);
    expect(visibleHistoryIds(entries, new Set(["batch"]), now)).toEqual([1, 2, 3, 4]);
    expect(visibleHistoryIds([], new Set(), now)).toEqual([]);
  });
});
