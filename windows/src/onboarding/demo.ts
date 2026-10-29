import demo from "../../../shared/onboarding/demo.json";
import type { HistoryCategory, HistoryEntry, HistoryPageQuery } from "../tailsyncClient";
import type { PreviewPayload } from "../utils/historyPreview";

type DemoSource = {
  id: number;
  name: string;
  category: HistoryEntry["category"];
  kind: "text" | "image" | "file";
  description: string;
  data_b64: string;
  batch_id?: string;
  batch_index?: number;
  batch_total?: number;
};

const records = (demo as { entries: DemoSource[] }).entries;
const recordById = new Map(records.map((record) => [record.id, record]));

export const DEMO_PEER_HOST = "DEMO-LAPTOP · 演示设备";

export function isDemoEntry(id: number): boolean {
  return recordById.has(id);
}

export function demoHistoryPage(
  query: HistoryPageQuery,
  hidden: ReadonlySet<string> = new Set(),
  favorites: ReadonlyMap<string, boolean> = new Map(),
): { entries: HistoryEntry[]; total: number; has_more: boolean } {
  const keyword = query.keyword?.trim().toLocaleLowerCase() ?? "";
  const entries = records
    .filter((record) => !hidden.has(demoLogicalKey(record)))
    .filter((record) => !query.category || record.category === query.category)
    .filter((record) => !keyword || record.description.toLocaleLowerCase().includes(keyword) || record.name.toLocaleLowerCase().includes(keyword))
    .filter((record) => query.collection !== "favorites" || (favorites.get(demoLogicalKey(record)) ?? record.category === "code"))
    .map((record) => ({
      id: record.id,
      timestamp: new Date(Date.now() - records.indexOf(record) * 37_000).toISOString(),
      type: record.batch_id ? "file" : record.kind,
      description: record.description,
      data_hash: `demo-${record.id}`,
      size_bytes: Math.floor(record.data_b64.length * 0.75),
      source_peer: record.id === records[0].id ? DEMO_PEER_HOST : "self",
      category: record.category as HistoryCategory,
      categories: [record.category as HistoryCategory],
      category_confidence: 100,
      classifier_version: 1,
      pinned: favorites.get(demoLogicalKey(record)) ?? record.category === "code",
      batch_id: record.batch_id ?? null,
      batch_index: record.batch_index ?? null,
      batch_total: record.batch_total ?? null,
      batch_count: record.batch_total ?? null,
      batch_status: "complete" as const,
      display: {
        cache_key: `demo-${record.id}`,
        kind: record.category === "image" ? "image" : record.name.match(/\.(docx|pptx)$/i) ? "document" : record.category === "file" ? "file" : "text",
        thumbnail_eligible: record.category === "image",
        version: 1,
      },
    }) satisfies HistoryEntry);
  const offset = query.offset ?? 0;
  const limit = query.limit ?? 30;
  return { entries: entries.slice(offset, offset + limit), total: entries.length, has_more: offset + limit < entries.length };
}

export function demoLogicalKey(entry: Pick<HistoryEntry, "id" | "batch_id">): string {
  return entry.batch_id ? `batch:${entry.batch_id}` : `entry:${entry.id}`;
}

export function demoRecordName(id: number): string | undefined {
  return recordById.get(id)?.name;
}

export function demoPreviewPayload(id: number, batchId?: string | null): PreviewPayload | null {
  const record = recordById.get(id);
  if (!record) return null;
  const bytes = Uint8Array.from(atob(record.data_b64), character => character.charCodeAt(0));
  const batch = batchId ?? record.batch_id;
  const siblings = batch
    ? records.filter(candidate => candidate.batch_id === batch).sort((left, right) => (left.batch_index ?? 0) - (right.batch_index ?? 0))
    : [];
  const index = siblings.findIndex(candidate => candidate.id === record.id);
  return {
    entry_id: record.id,
    kind: record.kind,
    name: record.name,
    size_bytes: bytes.length,
    width: null,
    height: null,
    batch: siblings.length > 1 && index >= 0 ? {
      batch_id: batch!, item_index: index, item_count: siblings.length,
      first_entry_id: siblings[0].id, last_entry_id: siblings.at(-1)!.id,
      previous_entry_id: index > 0 ? siblings[index - 1].id : null,
      next_entry_id: index + 1 < siblings.length ? siblings[index + 1].id : null,
    } : null,
    data: bytes,
  };
}
