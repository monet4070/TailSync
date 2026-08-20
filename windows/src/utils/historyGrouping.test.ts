import { describe, expect, it } from "vitest";
import type { HistoryEntry } from "../tailsyncClient";
import {
  computeBatchInfos,
  getDateGroup,
  groupEntriesByDate,
} from "./historyGrouping";

const NOW = new Date(2026, 7, 15, 12, 0, 0); // 2026-08-15 12:00 local

function entry(id: number, timestamp: string, batch?: string, batchCount?: number): HistoryEntry {
  return {
    id,
    timestamp,
    type: "text",
    description: `entry-${id}`,
    data_hash: `hash-${id}`,
    size_bytes: 1,
    source_peer: "peer",
    batch_id: batch ?? null,
    batch_total: batch ? 3 : undefined,
    batch_count: batchCount,
  };
}

describe("getDateGroup", () => {
  it("classifies today, yesterday, week, month and older", () => {
    expect(getDateGroup(new Date(2026, 7, 15, 9).toISOString(), NOW)).toBe("today");
    expect(getDateGroup(new Date(2026, 7, 14, 23).toISOString(), NOW)).toBe("yesterday");
    expect(getDateGroup(new Date(2026, 7, 9, 10).toISOString(), NOW)).toBe("thisWeek");
    expect(getDateGroup(new Date(2026, 7, 1, 10).toISOString(), NOW)).toBe("thisMonth");
    expect(getDateGroup(new Date(2026, 5, 1, 10).toISOString(), NOW)).toBe("older");
  });

  it("treats unparseable timestamps as older", () => {
    expect(getDateGroup("not-a-date", NOW)).toBe("older");
  });
});

describe("groupEntriesByDate", () => {
  it("returns groups in display order with only populated groups", () => {
    const entries = [
      entry(1, new Date(2026, 7, 15, 9).toISOString()),
      entry(2, new Date(2026, 7, 14, 9).toISOString()),
      entry(3, new Date(2026, 5, 1, 9).toISOString()),
    ];
    const groups = groupEntriesByDate(entries, NOW);
    expect(groups.map(([group]) => group)).toEqual(["today", "yesterday", "older"]);
    expect(groups[0][1].map((e) => e.id)).toEqual([1]);
  });

  it("returns an empty list for no entries", () => {
    expect(groupEntriesByDate([], NOW)).toEqual([]);
  });
});

describe("computeBatchInfos", () => {
  it("assigns positions and flags within a group", () => {
    const groups = groupEntriesByDate(
      [
        entry(1, new Date(2026, 7, 15, 9).toISOString(), "batch-a"),
        entry(2, new Date(2026, 7, 15, 9, 1).toISOString(), "batch-a"),
        entry(3, new Date(2026, 7, 15, 9, 2).toISOString()),
      ],
      NOW,
    );
    const infos = computeBatchInfos(groups);
    expect(infos.get(1)).toMatchObject({ batchId: "batch-a", batchPosition: 0, isBatchStart: true });
    expect(infos.get(2)).toMatchObject({ batchId: "batch-a", batchPosition: 1, isBatchStart: false });
    expect(infos.get(3)).toMatchObject({ batchId: null, batchPosition: 0, isBatchStart: false });
  });

  it("continues positions across groups but flags each group's first batch entry", () => {
    // A batch straddling midnight: entries on both Aug 14 (yesterday) and Aug 15 (today).
    const groups = groupEntriesByDate(
      [
        entry(1, new Date(2026, 7, 14, 23, 59).toISOString(), "batch-a"),
        entry(2, new Date(2026, 7, 15, 0, 1).toISOString(), "batch-a"),
        entry(3, new Date(2026, 7, 15, 0, 2).toISOString(), "batch-a"),
      ],
      NOW,
    );
    const infos = computeBatchInfos(groups);
    // Groups iterate in display order (today first), and positions continue
    // across groups via the shared counter — as in the page.
    expect(infos.get(2)?.batchPosition).toBe(0);
    expect(infos.get(3)?.batchPosition).toBe(1);
    expect(infos.get(1)?.batchPosition).toBe(2);
    // The group-local start flag is true for both groups' first batch entry.
    expect(infos.get(2)?.isBatchStart).toBe(true);
    expect(infos.get(1)?.isBatchStart).toBe(true);
    expect(infos.get(3)?.isBatchStart).toBe(false);
  });

  it("falls back batch count to total then one", () => {
    const groups = groupEntriesByDate(
      [
        entry(1, new Date(2026, 7, 15, 9).toISOString(), "batch-a", 2),
        entry(2, new Date(2026, 7, 15, 9, 1).toISOString(), "batch-a"),
        entry(3, new Date(2026, 7, 15, 9, 2).toISOString()),
      ],
      NOW,
    );
    const infos = computeBatchInfos(groups);
    expect(infos.get(1)?.batchCount).toBe(2);
    expect(infos.get(2)?.batchCount).toBe(3); // batch_total
    expect(infos.get(3)?.batchCount).toBe(1); // no batch
  });
});
