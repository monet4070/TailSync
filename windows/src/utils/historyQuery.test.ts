import { describe, expect, it } from "vitest";
import { buildHistoryQuery, type DateBoundsInput } from "./historyQuery";

const BOUNDS: DateBoundsInput = {
  start: new Date(2026, 7, 1).getTime(),
  end: new Date(2026, 7, 11).getTime(),
};

describe("buildHistoryQuery", () => {
  it("normalizes keyword and category", () => {
    const query = buildHistoryQuery("  notes  ", "text", BOUNDS, true, 30, 2);
    expect(query.keyword).toBe("notes");
    expect(query.category).toBe("text");

    const all = buildHistoryQuery("", "all", BOUNDS, true, 30, 0);
    expect(all.keyword).toBeNull();
    expect(all.category).toBeNull();
  });

  it("serializes date bounds when supported", () => {
    const query = buildHistoryQuery("", "all", BOUNDS, true, 30, 0);
    expect(query.startTime).toBe(new Date(2026, 7, 1).toISOString());
    expect(query.endTime).toBe(new Date(2026, 7, 11).toISOString());
  });

  it("drops date bounds when unsupported or absent", () => {
    const unsupported = buildHistoryQuery("", "all", BOUNDS, false, 30, 0);
    expect(unsupported.startTime).toBeNull();
    expect(unsupported.endTime).toBeNull();

    const empty = buildHistoryQuery("", "all", { start: null, end: null }, true, 30, 0);
    expect(empty.startTime).toBeNull();
    expect(empty.endTime).toBeNull();
  });

  it("computes limit and offset from the page", () => {
    const query = buildHistoryQuery("", "all", BOUNDS, true, 30, 3);
    expect(query.limit).toBe(30);
    expect(query.offset).toBe(90);
  });

  it("produces a payload accepted by getHistoryPage", () => {
    const query = buildHistoryQuery("x", "file", BOUNDS, true, 30, 1);
    expect(query).toMatchObject({
      keyword: "x",
      category: "file",
      limit: 30,
      offset: 30,
    });
    expect(typeof query.startTime).toBe("string");
  });
});
