import type { HistoryCategory, HistoryEntry } from "../../tailsyncClient";
import {
  HISTORY_ALWAYS_ON_TOP_STORAGE_KEY,
  HISTORY_CATEGORIES,
} from "./HistoryConstants";

export function readStoredHistoryAlwaysOnTop(): boolean | null {
  try {
    const value = localStorage.getItem(HISTORY_ALWAYS_ON_TOP_STORAGE_KEY);
    return value === null ? null : value === "true";
  } catch {
    return null;
  }
}

export function persistHistoryAlwaysOnTop(value: boolean): void {
  try {
    localStorage.setItem(HISTORY_ALWAYS_ON_TOP_STORAGE_KEY, String(value));
  } catch {
    // A restricted WebView may not provide persistent storage.
  }
}

export function resolvedCategory(entry: HistoryEntry): HistoryCategory {
  return entry.category && HISTORY_CATEGORIES.includes(entry.category)
    ? entry.category
    : entry.type;
}

export function resolvedCategories(entry: HistoryEntry): HistoryCategory[] {
  const primary = resolvedCategory(entry);
  const categories = (entry.categories ?? []).filter((category) =>
    HISTORY_CATEGORIES.includes(category),
  );
  return [primary, ...categories.filter((category) => category !== primary)].filter(
    (category, index, values) => values.indexOf(category) === index,
  );
}

export function formatTime(iso: string) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatSize(bytes: number) {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/**
 * Keyboard events from an editor belong to that editor, not the history
 * navigator.  In particular, the search field is auto-focused when this
 * window opens, so treating every Space key as a preview command would make
 * normal text entry impossible.
 */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT" ||
    target.tagName === "BUTTON" ||
    target.tagName === "A" ||
    Boolean(target.closest("[role='button'], [contenteditable]"))
  );
}

/**
 * A collapsed file batch represents the batch as a single logical item for
 * preview purposes.  Once the batch is expanded, each visible child keeps its
 * own identity and can be previewed independently.
 */
export function resolvePreviewEntryId(
  focusedId: number,
  entries: HistoryEntry[],
  expandedBatches: Set<string>,
): number | null {
  const focusedEntry = entries.find((entry) => entry.id === focusedId);
  if (!focusedEntry) return null;
  const batchId = focusedEntry.batch_id;
  if (!batchId || expandedBatches.has(batchId)) return focusedEntry.id;

  const firstBatchEntry = entries
    .filter((entry) => entry.batch_id === batchId)
    .sort((left, right) => {
      const leftIndex = left.batch_index ?? Number.MAX_SAFE_INTEGER;
      const rightIndex = right.batch_index ?? Number.MAX_SAFE_INTEGER;
      return leftIndex - rightIndex || left.id - right.id;
    })[0];
  return firstBatchEntry?.id ?? focusedEntry.id;
}
