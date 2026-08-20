// History date grouping and batch bookkeeping (T253 extraction).
//
// Pure helpers over the flat entry list. Batch positions continue across
// date groups (as in the original page), while the batch-start flag is
// group-local: the first batch entry of every group still renders a batch
// header, exactly like the original inline implementation.

import type { HistoryEntry } from "../tailsyncClient";

export type DateGroup = "today" | "yesterday" | "thisWeek" | "thisMonth" | "older";

export const GROUP_ORDER: DateGroup[] = [
  "today",
  "yesterday",
  "thisWeek",
  "thisMonth",
  "older",
];

export const GROUP_LABEL_KEYS: Record<DateGroup, string> = {
  today: "history.group.today",
  yesterday: "history.group.yesterday",
  thisWeek: "history.group.thisWeek",
  thisMonth: "history.group.thisMonth",
  older: "history.group.older",
};

export function getDateGroup(dateStr: string, now: Date): DateGroup {
  const d = new Date(dateStr);
  if (Number.isNaN(d.getTime())) return "older";
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const itemDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());

  if (itemDate.getTime() === today.getTime()) return "today";
  if (itemDate.getTime() === yesterday.getTime()) return "yesterday";
  // Compare local calendar dates through UTC ordinals so DST transitions do
  // not turn a seven-day boundary into 6.96 or 7.04 elapsed days.
  const dayOrdinal = (date: Date) =>
    Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) /
    (1000 * 60 * 60 * 24);
  const diffDays = dayOrdinal(today) - dayOrdinal(itemDate);
  if (diffDays <= 7) return "thisWeek";
  if (diffDays <= 30) return "thisMonth";
  return "older";
}

export type GroupedEntries = Array<[DateGroup, HistoryEntry[]]>;

export function groupEntriesByDate(entries: HistoryEntry[], now: Date): GroupedEntries {
  const groups: Partial<Record<DateGroup, HistoryEntry[]>> = {};
  for (const entry of entries) {
    const group = getDateGroup(entry.timestamp, now);
    (groups[group] ??= []).push(entry);
  }
  return GROUP_ORDER
    .filter((group) => groups[group])
    .map((group) => [group, groups[group]!]);
}

export interface BatchInfo {
  batchId: string | null;
  batchPosition: number;
  batchTotal: number;
  batchCount: number;
  isBatchStart: boolean;
}

/// Batch bookkeeping for every entry, keyed by entry id. Positions continue
/// across groups; the batch-start flag is group-local.
export function computeBatchInfos(groups: GroupedEntries): Map<number, BatchInfo> {
  const positions = new Map<string, number>();
  const infos = new Map<number, BatchInfo>();
  for (const [, groupEntries] of groups) {
    groupEntries.forEach((entry, index) => {
      const batchId = entry.batch_id ?? null;
      const batchPosition = batchId ? (positions.get(batchId) ?? 0) : 0;
      if (batchId) positions.set(batchId, batchPosition + 1);
      const batchTotal = entry.batch_total ?? 1;
      infos.set(entry.id, {
        batchId,
        batchPosition,
        batchTotal,
        batchCount: entry.batch_count ?? batchTotal,
        isBatchStart: Boolean(batchId && groupEntries[index - 1]?.batch_id !== batchId),
      });
    });
  }
  return infos;
}
