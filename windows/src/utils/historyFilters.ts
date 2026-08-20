// History date-filter helpers (T250 extraction from History.tsx).
//
// Pure functions over local calendar dates; no React, no I/O. Date parsing
// accepts `YYYY-MM-DD` and rejects invalid calendar days (including
// rollovers like 2023-02-30).

export type DateFilter =
  | "all"
  | "today"
  | "yesterday"
  | "last7"
  | "last30"
  | "thisMonth"
  | "custom";

export const DATE_FILTER_OPTIONS: DateFilter[] = [
  "all",
  "today",
  "yesterday",
  "last7",
  "last30",
  "thisMonth",
  "custom",
];

export function localDateFromInput(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > 31) {
    return null;
  }

  // setFullYear avoids JavaScript's special 1900 offset for years 0-99.
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(year, month - 1, day);
  if (
    Number.isNaN(date.getTime()) ||
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return null;
  }
  return date;
}

export function dateInputValue(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function localCalendarContextKey(date: Date): string {
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
  return `${dateInputValue(date)}|${date.getTimezoneOffset()}|${timeZone}`;
}

export interface DateBounds {
  start: number | null;
  end: number | null;
  valid: boolean;
}

export function dateBounds(
  filter: DateFilter,
  customStart: string,
  customEnd: string,
  now: Date,
): DateBounds {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);
  if (filter === "all") return { start: null, end: null, valid: true };
  if (filter === "today") return { start: today.getTime(), end: tomorrow.getTime(), valid: true };
  if (filter === "yesterday") {
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    return { start: yesterday.getTime(), end: today.getTime(), valid: true };
  }
  if (filter === "last7" || filter === "last30") {
    const start = new Date(today);
    start.setDate(start.getDate() - (filter === "last7" ? 6 : 29));
    return { start: start.getTime(), end: tomorrow.getTime(), valid: true };
  }
  if (filter === "thisMonth") {
    const start = new Date(today.getFullYear(), today.getMonth(), 1);
    const end = new Date(today.getFullYear(), today.getMonth() + 1, 1);
    return { start: start.getTime(), end: end.getTime(), valid: true };
  }
  const start = customStart ? localDateFromInput(customStart) : null;
  const inclusiveEnd = customEnd ? localDateFromInput(customEnd) : null;
  const end = inclusiveEnd ? new Date(inclusiveEnd) : null;
  if (end) end.setDate(end.getDate() + 1);
  const validInputs = (!customStart || start !== null) && (!customEnd || inclusiveEnd !== null);
  const ordered = !start || !inclusiveEnd || start.getTime() <= inclusiveEnd.getTime();
  return {
    start: start?.getTime() ?? null,
    end: end?.getTime() ?? null,
    valid: Boolean((customStart || customEnd) && validInputs && ordered),
  };
}
