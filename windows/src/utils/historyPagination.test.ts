import { describe, expect, it } from "vitest";
import {
  HISTORY_PAGE_SIZE,
  historyPageCount,
  normalizeHistoryPage,
} from "./historyPagination";

describe("history pagination", () => {
  it("keeps empty results on the first page", () => {
    expect(historyPageCount(0)).toBe(1);
    expect(normalizeHistoryPage(4, 0)).toBe(0);
  });

  it("preserves pages that still contain results", () => {
    expect(historyPageCount(HISTORY_PAGE_SIZE * 2 + 1)).toBe(3);
    expect(normalizeHistoryPage(1, HISTORY_PAGE_SIZE * 2 + 1)).toBe(1);
  });

  it("moves an out-of-range request to the last available page", () => {
    expect(normalizeHistoryPage(8, HISTORY_PAGE_SIZE + 1)).toBe(1);
    expect(normalizeHistoryPage(-3, 100)).toBe(0);
  });

  it("rejects invalid page sizes", () => {
    expect(() => historyPageCount(10, 0)).toThrow("positive integer");
  });
});
