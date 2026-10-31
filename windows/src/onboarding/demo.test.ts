import { describe, expect, it } from "vitest";
import { demoHistoryPage, demoLogicalKey, demoPreviewPayload, demoDocumentFrame, isDemoDocument, isDemoEntry } from "./demo";
import { previewRendererFor } from "../utils/historyPreview";

const query = { keyword: null, category: null, startTime: null, endTime: null, limit: 100, offset: 0 };

describe("onboarding demo data", () => {
  it("supplies a PPTX document frame with request identity while retaining the DOCX renderer", () => {
    const request = { entry_id: 8_000_000_012, owner: "history" as const, generation: 42, request_id: "demo-request" };
    const frame = demoDocumentFrame(request)!;
    expect(isDemoDocument(request.entry_id)).toBe(true);
    expect(frame.metadata).toMatchObject({ ...request, name: "Demo.pptx", kind: "document" });
    expect(frame.document.blocks.length).toBeGreaterThan(0);
    expect(frame.assets.byteLength).toBe(0);
    expect(isDemoDocument(8_000_000_011)).toBe(false);
    expect(previewRendererFor(demoPreviewPayload(8_000_000_011)!)).toBe("docx");
    expect(demoDocumentFrame({ ...request, entry_id: 7 })).toBeNull();
  });
  it("covers every history category without touching persisted state", () => {
    const page = demoHistoryPage(query);
    expect(page.entries).toHaveLength(17);
    expect(new Set(page.entries.map(entry => entry.category))).toEqual(new Set(["text", "website", "code", "path", "image", "file"]));
    expect(page.entries[0].description).toContain("欢迎使用 TailSync");
    expect(page.entries.filter(entry => entry.source_peer.startsWith("DEMO-LAPTOP"))).toHaveLength(1);
  });

  it("filters searches, favorites and batches independently", () => {
    expect(demoHistoryPage({ ...query, keyword: "TailSync" }).entries).toHaveLength(1);
    expect(demoHistoryPage({ ...query, category: "website" }).entries).toHaveLength(1);
    expect(demoHistoryPage({ ...query, collection: "favorites" }).entries.every(entry => entry.pinned)).toBe(true);
    const batch = demoHistoryPage({ ...query, category: "file" }).entries.find(entry => entry.batch_id === "tailsync-demo-batch")!;
    const preview = demoPreviewPayload(batch.id, batch.batch_id)!;
    expect(preview.batch).toEqual(expect.objectContaining({ batch_id: "tailsync-demo-batch", item_count: 3 }));
    expect(demoLogicalKey(batch)).toBe("batch:tailsync-demo-batch");
  });

  it("serves real preview bytes from the bundled sample", () => {
    const png = demoHistoryPage(query).entries.find(entry => entry.description.includes("图片 / Image"))!;
    const preview = demoPreviewPayload(png.id)!;
    expect(isDemoEntry(png.id)).toBe(true);
    expect(preview.name).toBe("Demo.png");
    expect(Array.from(preview.data.slice(0, 8))).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
  });
});
