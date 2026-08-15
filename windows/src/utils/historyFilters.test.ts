import { describe, expect, it } from "vitest";
import {
  DATE_FILTER_OPTIONS,
  dateBounds,
  dateInputValue,
  localCalendarContextKey,
  localDateFromInput,
} from "./historyFilters";

const NOW = new Date(2026, 7, 15, 13, 30, 0); // 2026-08-15 13:30 local

describe("localDateFromInput", () => {
  it("parses valid local dates", () => {
    const date = localDateFromInput("2026-08-15");
    expect(date).not.toBeNull();
    expect(date!.getFullYear()).toBe(2026);
    expect(date!.getMonth()).toBe(7);
    expect(date!.getDate()).toBe(15);
  });

  it("rejects malformed and out-of-range input", () => {
    expect(localDateFromInput("")).toBeNull();
    expect(localDateFromInput("2026-8-5")).toBeNull();
    expect(localDateFromInput("2026-13-01")).toBeNull();
    expect(localDateFromInput("2026-00-10")).toBeNull();
    expect(localDateFromInput("2026-02-30")).toBeNull();
    expect(localDateFromInput("abc")).toBeNull();
  });

  it("handles years below 100 without the 1900 offset", () => {
    const date = localDateFromInput("0099-01-02");
    expect(date).not.toBeNull();
    expect(date!.getFullYear()).toBe(99);
  });
});

describe("dateInputValue", () => {
  it("round-trips zero-padded values", () => {
    expect(dateInputValue(new Date(2026, 0, 5))).toBe("2026-01-05");
    expect(dateInputValue(new Date(2026, 11, 31))).toBe("2026-12-31");
    const parsed = localDateFromInput("2026-08-15");
    expect(dateInputValue(parsed!)).toBe("2026-08-15");
  });
});

describe("localCalendarContextKey", () => {
  it("includes the date, offset and timezone", () => {
    const key = localCalendarContextKey(new Date(2026, 7, 15));
    expect(key).toMatch(/^2026-08-15\|-?\d+\|/);
  });
});

describe("dateBounds", () => {
  it("returns null bounds for 'all'", () => {
    expect(dateBounds("all", "", "", NOW)).toEqual({
      start: null,
      end: null,
      valid: true,
    });
  });

  it("bounds today as [start-of-day, start-of-tomorrow)", () => {
    const bounds = dateBounds("today", "", "", NOW);
    expect(bounds.valid).toBe(true);
    expect(bounds.start).toBe(new Date(2026, 7, 15).getTime());
    expect(bounds.end).toBe(new Date(2026, 7, 16).getTime());
  });

  it("bounds yesterday as the previous calendar day", () => {
    const bounds = dateBounds("yesterday", "", "", NOW);
    expect(bounds.start).toBe(new Date(2026, 7, 14).getTime());
    expect(bounds.end).toBe(new Date(2026, 7, 15).getTime());
  });

  it("bounds last7 and last30 relative to today", () => {
    const last7 = dateBounds("last7", "", "", NOW);
    expect(last7.start).toBe(new Date(2026, 7, 9).getTime());
    const last30 = dateBounds("last30", "", "", NOW);
    expect(last30.start).toBe(new Date(2026, 6, 17).getTime());
  });

  it("bounds thisMonth from the first of the month", () => {
    const bounds = dateBounds("thisMonth", "", "", NOW);
    expect(bounds.start).toBe(new Date(2026, 7, 1).getTime());
    expect(bounds.end).toBe(new Date(2026, 8, 1).getTime());
  });

  it("validates custom ranges", () => {
    const valid = dateBounds("custom", "2026-08-01", "2026-08-10", NOW);
    expect(valid.valid).toBe(true);
    expect(valid.start).toBe(new Date(2026, 7, 1).getTime());
    // The end is exclusive: inclusive date + 1 day.
    expect(valid.end).toBe(new Date(2026, 7, 11).getTime());

    const reversed = dateBounds("custom", "2026-08-10", "2026-08-01", NOW);
    expect(reversed.valid).toBe(false);

    const badInput = dateBounds("custom", "2026-02-30", "2026-08-01", NOW);
    expect(badInput.valid).toBe(false);

    const partial = dateBounds("custom", "2026-08-01", "", NOW);
    expect(partial.valid).toBe(true);
    expect(partial.end).toBeNull();
  });
});

describe("DATE_FILTER_OPTIONS", () => {
  it("lists every filter in render order", () => {
    expect(DATE_FILTER_OPTIONS).toEqual([
      "all",
      "today",
      "yesterday",
      "last7",
      "last30",
      "thisMonth",
      "custom",
    ]);
  });
});
