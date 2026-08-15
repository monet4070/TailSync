// History query building (T252 extraction from History.tsx).
//
// Pure function turning filter state into the get_history_page payload.
// Date bounds are millisecond timestamps; the capability flag suppresses
// date filtering entirely (null times) when the peer does not support it.

import type { HistoryCategory, HistoryPageQuery } from "../tailsyncClient";

export interface DateBoundsInput {
  start: number | null;
  end: number | null;
}

export function buildHistoryQuery(
  keyword: string,
  category: HistoryCategory | "all",
  dateBounds: DateBoundsInput,
  dateFilteringSupported: boolean,
  pageSize: number,
  page: number,
): HistoryPageQuery {
  const startTime = dateFilteringSupported && dateBounds.start !== null
    ? new Date(dateBounds.start).toISOString()
    : null;
  const endTime = dateFilteringSupported && dateBounds.end !== null
    ? new Date(dateBounds.end).toISOString()
    : null;
  return {
    keyword: keyword.trim() || null,
    category: category === "all" ? null : category,
    startTime,
    endTime,
    limit: pageSize,
    offset: page * pageSize,
  };
}
