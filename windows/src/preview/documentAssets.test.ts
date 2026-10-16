import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decodeDocumentFrame, type DecodedDocumentFrame } from "../utils/documentPreview";
import {
  buildPngAsset,
  concatAssets,
  documentAsset,
  documentImageBlock,
  documentParagraph,
  documentRun,
  encodeDocumentFrame,
  type BuiltDocumentAsset,
  type DocumentFrameControl,
} from "../test/documentFrames";
import {
  DOCUMENT_FRONTEND_MAX_PLACEMENT_PIXELS,
  DocumentPrepareError,
  groupDocumentBlocks,
  planDocumentImages,
  prepareDocumentAssets,
} from "./documentAssets";

const control: DocumentFrameControl = {
  entry_id: 7,
  owner: "history",
  generation: 23,
  request_id: "assets-test",
};
const request: import("../types/localContracts.generated").DocumentRequest = {
  entry_id: 7,
  owner: "history",
  generation: 23,
  request_id: "assets-test",
};

function decode(document: Record<string, unknown>, assets: BuiltDocumentAsset[]): DecodedDocumentFrame {
  return decodeDocumentFrame(encodeDocumentFrame(control, document, concatAssets(assets)), request);
}

describe("documentAssets", () => {
  beforeEach(() => {
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:asset-${counter++}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  });

  afterEach(() => vi.restoreAllMocks());

  it("keeps declared image placements in document order", () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0x11));
    const blue = documentAsset(1, red.bytes.byteLength, buildPngAsset(4, 4, 0x22));
    const frame = decode({
      version: 1,
      blocks: [
        documentParagraph([documentRun("intro")]),
        documentImageBlock(0, 0),
        documentParagraph([documentRun("between")]),
        documentImageBlock(1, 1),
      ],
      assets: [red.asset, blue.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red, blue]);

    const plan = planDocumentImages(frame.document, new Map([[0, "blob:red"], [1, "blob:blue"]]));
    const entries = [...plan.placements.entries()];
    expect(entries.map(([blockIndex]) => blockIndex)).toEqual([1, 3]);
    expect(entries[0][1]).toEqual({ kind: "image", assetIndex: 0, url: "blob:red" });
    expect(entries[1][1]).toEqual({ kind: "image", assetIndex: 1, url: "blob:blue" });
    expect(plan.pixelBudgetExceeded).toBe(false);
  });

  it("charges repeated placements the full decoded cost instead of trusting one declared asset", () => {
    const large = documentAsset(0, 0, buildPngAsset(2048, 2048, 0x33));
    const frame = decode({
      version: 1,
      blocks: Array.from({ length: 5 }, (_, ordinal) => documentImageBlock(0, ordinal)),
      assets: [large.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [large]);

    const plan = planDocumentImages(frame.document, new Map([[0, "blob:large"]]));
    const rendered = [...plan.placements.values()].filter((entry) => entry.kind === "image");
    const placeholders = [...plan.placements.values()].filter((entry) => entry.kind === "placeholder");
    expect(rendered).toHaveLength(3);
    expect(placeholders).toHaveLength(2);
    expect(placeholders.every((entry) => entry.kind === "placeholder" && entry.budgetLimited)).toBe(true);
    expect(plan.pixelBudgetExceeded).toBe(true);
    expect(2048 * 2048 * rendered.length).toBeLessThanOrEqual(DOCUMENT_FRONTEND_MAX_PLACEMENT_PIXELS);
  });

  it("shows an honest placeholder for an unavailable first image without promoting the second", () => {
    const blue = documentAsset(0, 0, buildPngAsset(2, 2, 0x44));
    const frame = decode({
      version: 1,
      blocks: [documentImageBlock(null, 0, "external_reference"), documentImageBlock(0, 1)],
      assets: [blue.asset],
      partial: [{ code: "external_reference", detail: "first image unavailable" }],
      styles_partial: true,
      first_image: null,
    }, [blue]);

    const plan = planDocumentImages(frame.document, new Map([[0, "blob:blue"]]));
    expect(plan.placements.get(0)).toEqual({
      kind: "placeholder",
      availability: "external_reference",
      budgetLimited: false,
    });
    expect(plan.placements.get(1)).toEqual({ kind: "image", assetIndex: 0, url: "blob:blue" });
  });

  it("retains one session-private URL per declared asset and revokes exactly those", async () => {
    const red = documentAsset(0, 0, buildPngAsset(3, 3, 0x55));
    const blue = documentAsset(1, red.bytes.byteLength, buildPngAsset(3, 3, 0x66));
    const frame = decode({
      version: 1,
      blocks: [documentImageBlock(0, 0), documentImageBlock(1, 1)],
      assets: [red.asset, blue.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red, blue]);

    const prepared = await prepareDocumentAssets(frame);
    expect([...prepared.urls.keys()]).toEqual([0, 1]);
    expect(prepared.urls.get(0)).toBe("blob:asset-0");
    expect(prepared.urls.get(1)).toBe("blob:asset-1");
    expect(prepared.retainedBytes).toBeGreaterThanOrEqual(concatAssets([red, blue]).byteLength);

    prepared.release();
    expect(vi.mocked(URL.revokeObjectURL).mock.calls.map(([url]) => url)).toEqual(["blob:asset-0", "blob:asset-1"]);
    expect(prepared.urls.size).toBe(0);
  });

  it("revokes already-created URLs when preparation is cancelled between assets", async () => {
    const red = documentAsset(0, 0, buildPngAsset(3, 3, 0x77));
    const blue = documentAsset(1, red.bytes.byteLength, buildPngAsset(3, 3, 0x88));
    const frame = decode({
      version: 1,
      blocks: [documentImageBlock(0, 0), documentImageBlock(1, 1)],
      assets: [red.asset, blue.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red, blue]);

    const signal = { aborted: false };
    vi.mocked(URL.createObjectURL).mockImplementation(() => {
      signal.aborted = true;
      return "blob:first";
    });

    await expect(prepareDocumentAssets(frame, signal as AbortSignal)).rejects.toMatchObject({
      name: "DocumentPrepareError",
      code: "cancelled",
    });
    expect(vi.mocked(URL.revokeObjectURL).mock.calls.map(([url]) => url)).toEqual(["blob:first"]);
  });

  it("charges parsed-model and blob costs beyond the raw asset bytes", async () => {
    const red = documentAsset(0, 0, buildPngAsset(3, 3, 0x99));
    const frame = decode({
      version: 1,
      blocks: [documentImageBlock(0, 0)],
      assets: [red.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red]);

    await expect(prepareDocumentAssets(frame, undefined, { maxRetainedBytes: frame.assets.buffer.byteLength })).rejects.toBeInstanceOf(DocumentPrepareError);
    expect(URL.createObjectURL).not.toHaveBeenCalled();
  });

  it("rejects an already cancelled assetless frame", async () => {
    const frame = decode({ version: 1, blocks: [], assets: [], partial: [], styles_partial: false }, []);
    const controller = new AbortController();
    controller.abort();
    await expect(prepareDocumentAssets(frame, controller.signal)).rejects.toMatchObject({ code: "cancelled" });
  });

  it("groups consecutive list paragraphs without disturbing other blocks", () => {
    const groups = groupDocumentBlocks([
      documentParagraph([documentRun("a")], "bullet"),
      documentParagraph([documentRun("b")], "bullet"),
      documentParagraph([documentRun("c")]),
      documentParagraph([documentRun("d")], "ordered"),
      documentImageBlock(null, 0, "missing_resource"),
      { kind: "table", table: { rows: [{ cells: [{ header: true, runs: [] }] }], truncated: false } },
    ] as never);
    expect(groups.map((group) => group.kind)).toEqual(["list", "paragraph", "list", "image", "table"]);
    const firstList = groups[0];
    expect(firstList.kind === "list" && firstList.items).toHaveLength(2);
    expect(firstList.kind === "list" && firstList.list).toBe("bullet");
  });

  it("keeps different list depths, quote depths and code blocks in separate groups", () => {
    const groups = groupDocumentBlocks([
      { kind: "paragraph", paragraph: { runs: [], list: "bullet", list_depth: 0 } },
      { kind: "paragraph", paragraph: { runs: [], list: "bullet", list_depth: 0 } },
      { kind: "paragraph", paragraph: { runs: [], list: "bullet", list_depth: 1 } },
      { kind: "paragraph", paragraph: { runs: [], list: "bullet", list_depth: 0 } },
      { kind: "paragraph", paragraph: { runs: [], quote_depth: 1 } },
      { kind: "paragraph", paragraph: { runs: [], quote_depth: 1 } },
      { kind: "paragraph", paragraph: { runs: [], quote_depth: 2 } },
      { kind: "paragraph", paragraph: { runs: [], code_block: true } },
      { kind: "paragraph", paragraph: { runs: [] } },
    ] as never);

    expect(groups.map((group) => group.kind)).toEqual([
      "list", "list", "list", "quote", "quote", "paragraph", "paragraph",
    ]);
    expect(groups.map((group) => group.kind === "list" ? group.depth : null)).toEqual([0, 1, 0, null, null, null, null]);
    expect(groups.map((group) => group.kind === "list" ? group.items.length : 0)).toEqual([2, 1, 1, 0, 0, 0, 0]);
    expect(groups.map((group) => group.kind === "quote" ? group.depth : null)).toEqual([null, null, null, 1, 2, null, null]);
    expect(groups.map((group) => group.kind === "quote" ? group.items.length : 0)).toEqual([0, 0, 0, 2, 1, 0, 0]);
    expect(groups[5].kind === "paragraph" && groups[5].block.paragraph?.code_block).toBe(true);
  });

  it("keeps quoted and unquoted list items apart even when the list kind matches", () => {
    const groups = groupDocumentBlocks([
      { kind: "paragraph", paragraph: { runs: [], list: "ordered", list_depth: 0, quote_depth: 0 } },
      { kind: "paragraph", paragraph: { runs: [], list: "ordered", list_depth: 0, quote_depth: 1 } },
      { kind: "paragraph", paragraph: { runs: [], list: "ordered", list_depth: 0, quote_depth: 1 } },
      { kind: "paragraph", paragraph: { runs: [], list: "ordered", list_depth: 0, quote_depth: 0 } },
    ] as never);
    expect(groups.map((group) => group.kind)).toEqual(["list", "list", "list"]);
    expect(groups.map((group) => group.kind === "list" ? group.quoteDepth : null)).toEqual([0, 1, 0]);
    expect(groups.map((group) => group.kind === "list" ? group.items.length : 0)).toEqual([1, 2, 1]);
  });
});
