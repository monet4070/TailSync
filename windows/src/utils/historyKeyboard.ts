import type { HistoryEntry } from "../tailsyncClient";
import { computeBatchInfos, groupEntriesByDate } from "./historyGrouping";
import { COLLAPSED_BATCH_FILE_LIMIT } from "../pages/history/HistoryConstants";

export function isHistoryListTarget(target: EventTarget | null): target is HTMLElement {
  return target instanceof HTMLElement && target.matches(".history-list, .history-item");
}

export function historyShortcutNumber(key: string): number | null {
  return /^[1-6]$/.test(key) ? Number(key) : null;
}

// Some WebView2 IMEs emit a final Enter/Escape after compositionend.
export class HistoryImeGuard {
  private composing = false;
  private graceUntil = 0;
  start() { this.composing = true; }
  end(now = Date.now()) { this.composing = false; this.graceUntil = now + 100; }
  blocks(event: Pick<KeyboardEvent, "isComposing" | "keyCode">, now = Date.now()) {
    return this.composing || event.isComposing || event.keyCode === 229 || now < this.graceUntil;
  }
}

export function visibleHistoryIds(entries: HistoryEntry[], expanded: Set<string>, now: Date): number[] {
  const groups = groupEntriesByDate(entries, now);
  const infos = computeBatchInfos(groups);
  return groups.flatMap(([, items]) => items.filter((entry) => {
    const info = infos.get(entry.id)!;
    return !info.batchId || expanded.has(info.batchId) || info.batchPosition < COLLAPSED_BATCH_FILE_LIMIT;
  }).map((entry) => entry.id));
}
