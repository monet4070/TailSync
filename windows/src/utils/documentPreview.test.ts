import { beforeEach, describe, expect, it, vi } from "vitest";
import { cancelDocumentPreview, getDocumentPreview } from "../tailsyncClient";
import {
  DOCUMENT_HEADER_BYTES,
  DOCUMENT_MAX_METADATA_BYTES,
  DOCUMENT_RESPONSE_VERSION,
  DocumentParseError,
  decodeDocumentFrame,
  documentAssetBytes,
  type DocumentResponseInput,
} from "./documentPreview";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../diagnostics/localIpcMetrics", () => ({ invoke: bridge.invoke }));
const request = { entry_id: 7, owner: "history" as const, generation: 1, request_id: "fixture-request" };

type DocumentMetadata = {
  request_id?: string | null;
  entry_id: number;
  kind: string;
  name: string;
  stamp: string;
  size_bytes: number;
  document: Record<string, unknown>;
  batch?: unknown;
};

function style() {
  return {
    bold: false,
    italic: false,
    underline: false,
    strikethrough: false,
    subscript: false,
    superscript: false,
  };
}

function paragraphDocument(assets: Array<Record<string, unknown>> = []) {
  return {
    version: 1,
    blocks: [
      {
        kind: "paragraph",
        paragraph: {
          runs: [{ text: "hello", style: style(), unsupported: false }],
        },
      },
      ...assets.map((asset, ordinal) => ({ kind: "image", image: { asset: asset.index, availability: "available", ordinal } })),
    ],
    assets,
    partial: [],
    styles_partial: false,
    ...(assets.length ? { first_image: 0 } : {}),
  };
}

function asset(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    index: 0,
    offset: 0,
    length: 3,
    width: 1,
    height: 1,
    sha256: "00".repeat(32),
    ...overrides,
  };
}

function encodeDocument(metadata: DocumentMetadata, payload: Uint8Array): Uint8Array {
  const model = new TextEncoder().encode(JSON.stringify(metadata.document));
  const { document, ...control } = metadata;
  void document;
  const metadataBytes = new TextEncoder().encode(JSON.stringify({ ...control, owner: "history", generation: 1, model_bytes: model.length, size_bytes: metadata.size_bytes + model.length }));
  const frame = new Uint8Array(DOCUMENT_HEADER_BYTES + metadataBytes.length + model.length + payload.length);
  frame.set(new TextEncoder().encode("TSDC"), 0);
  frame[4] = DOCUMENT_RESPONSE_VERSION;
  new DataView(frame.buffer).setUint32(5, metadataBytes.length, true);
  frame.set(metadataBytes, DOCUMENT_HEADER_BYTES);
  frame.set(model, DOCUMENT_HEADER_BYTES + metadataBytes.length);
  frame.set(payload, DOCUMENT_HEADER_BYTES + metadataBytes.length + model.length);
  return frame;
}

function metadata(overrides: Partial<DocumentMetadata> = {}): DocumentMetadata {
  return {
    request_id: "fixture-request",
    entry_id: 7,
    kind: "document",
    name: "composite.txt",
    stamp: `dc1:${"0a".repeat(32)}`,
    size_bytes: 0,
    document: paragraphDocument(),
    ...overrides,
  };
}

function expectParseError(input: DocumentResponseInput, code: DocumentParseError["code"]) {
  try {
    decodeDocumentFrame(input);
    throw new Error("expected decoder to reject the frame");
  } catch (error) {
    expect(error).toBeInstanceOf(DocumentParseError);
    expect(error).toMatchObject({ code });
  }
}

describe("decodeDocumentFrame", () => {
  it("accepts a valid document frame with a raw asset payload", () => {
    const payload = new Uint8Array([1, 2, 3]);
    const frame = encodeDocument(
      metadata({ size_bytes: payload.length, document: paragraphDocument([asset()]) }),
      payload,
    );
    const decoded = decodeDocumentFrame(frame);
    expect(decoded.metadata.kind).toBe("document");
    expect(decoded.metadata.entry_id).toBe(7);
    expect(decoded.assets).toEqual(payload);
  });

  it("rejects an owner, generation, entry or request mismatch", () => {
    const frame = encodeDocument(metadata(), new Uint8Array());
    for (const changed of [{ ...request, owner: "favorites" as const }, { ...request, generation: 4 }, { ...request, entry_id: 8 }, { ...request, request_id: "old" }]) {
      expect(() => decodeDocumentFrame(frame, changed)).toThrow();
    }
  });

  it("rejects an unknown magic or version", () => {
    const frame = encodeDocument(metadata(), new Uint8Array());
    const badMagic = frame.slice();
    badMagic[0] = 0x58;
    expectParseError(badMagic, "invalid_magic");
    const badVersion = frame.slice();
    badVersion[4] = 2;
    expectParseError(badVersion, "unsupported_version");
  });

  it("rejects an unknown document kind", () => {
    const frame = encodeDocument(metadata({ kind: "text" }), new Uint8Array());
    expectParseError(frame, "invalid_metadata");
  });

  it("rejects an invalid or truncated metadata length", () => {
    const frame = encodeDocument(metadata(), new Uint8Array());
    const zero = frame.slice();
    new DataView(zero.buffer).setUint32(5, 0, true);
    expectParseError(zero, "invalid_metadata_length");
    const oversized = frame.slice();
    new DataView(oversized.buffer).setUint32(5, DOCUMENT_MAX_METADATA_BYTES + 1, true);
    expectParseError(oversized, "invalid_metadata_length");
    const truncated = frame.slice(0, DOCUMENT_HEADER_BYTES + 2);
    new DataView(truncated.buffer).setUint32(5, 512, true);
    expectParseError(truncated, "truncated_header");
  });

  it("rejects a payload length mismatch", () => {
    const frame = encodeDocument(metadata({ size_bytes: 5 }), new Uint8Array([1, 2, 3]));
    expectParseError(frame, "invalid_assets");
  });

  it("rejects non-contiguous or unordered assets", () => {
    const payload = new Uint8Array([1, 2, 3, 4, 5, 6]);
    const overlapping = encodeDocument(
      metadata({
        size_bytes: payload.length,
        document: paragraphDocument([
          asset({ index: 0, offset: 0, length: 3 }),
          asset({ index: 1, offset: 1, length: 3 }),
        ]),
      }),
      payload,
    );
    expectParseError(overlapping, "invalid_metadata");
    const misordered = encodeDocument(
      metadata({
        size_bytes: payload.length,
        document: paragraphDocument([
          asset({ index: 1, offset: 0, length: 3 }),
          asset({ index: 0, offset: 3, length: 3 }),
        ]),
      }),
      payload,
    );
    expectParseError(misordered, "invalid_metadata");
  });
});

describe("documentAssetBytes", () => {
  it("returns a slice only when the digest matches", async () => {
    const payload = new Uint8Array([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1]);
    const digest = await crypto.subtle.digest("SHA-256", payload);
    const sha256 = Array.from(new Uint8Array(digest))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
    const frame = encodeDocument(
      metadata({
        size_bytes: payload.length,
        document: paragraphDocument([asset({ sha256, length: payload.length })]),
      }),
      payload,
    );
    const decoded = decodeDocumentFrame(frame);
    expect(await documentAssetBytes(decoded, decoded.document.assets[0])).toEqual(payload);
    const tampered = { ...decoded.document.assets[0], sha256: "00".repeat(32) };
    expect(await documentAssetBytes(decoded, tampered)).toBeNull();
  });
});

describe("document production client lifecycle", () => {
  beforeEach(() => bridge.invoke.mockReset());

  it("revalidates backend identity before returning installable data", async () => {
    bridge.invoke.mockResolvedValueOnce(encodeDocument(metadata(), new Uint8Array())).mockResolvedValueOnce(true);
    const loaded = await getDocumentPreview(request);
    expect(loaded.metadata.owner).toBe("history");
    expect(bridge.invoke).toHaveBeenNthCalledWith(2, "validate_document_preview", { request, stamp: loaded.metadata.stamp });
  });

  it("rejects a stale database identity instead of returning the frame", async () => {
    bridge.invoke.mockResolvedValueOnce(encodeDocument(metadata(), new Uint8Array())).mockResolvedValueOnce(false).mockResolvedValue(undefined);
    await expect(getDocumentPreview(request)).rejects.toThrow("stale");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
  });

  it("cancel and late delivery never return installable data", async () => {
    let resolve: (bytes: Uint8Array) => void = () => undefined;
    bridge.invoke.mockImplementation((command: string) => command === "get_document_preview" ? new Promise<Uint8Array>((ready) => { resolve = ready; }) : Promise.resolve(undefined));
    const cancellation = new AbortController();
    const loading = getDocumentPreview(request, cancellation.signal);
    await vi.waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("get_document_preview", { request }));
    cancellation.abort(); resolve(encodeDocument(metadata(), new Uint8Array()));
    await expect(loading).rejects.toThrow("cancelled");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
    expect(bridge.invoke).not.toHaveBeenCalledWith("validate_document_preview", expect.anything());
  });

  it("rejects cancellation while backend installation validation is pending", async () => {
    let validated: (current: boolean) => void = () => undefined;
    bridge.invoke.mockImplementation((command: string) => {
      if (command === "get_document_preview") return Promise.resolve(encodeDocument(metadata(), new Uint8Array()));
      if (command === "validate_document_preview") return new Promise<boolean>((ready) => { validated = ready; });
      return Promise.resolve(undefined);
    });
    const cancellation = new AbortController();
    const loading = getDocumentPreview(request, cancellation.signal);
    await vi.waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("validate_document_preview", expect.anything()));
    cancellation.abort(); validated(true);
    await expect(loading).rejects.toThrow("cancelled");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
  });

  it("keeps one running cancellation and at most the newest request per owner", async () => {
    let finish!: () => void;
    bridge.invoke.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
    const running = cancelDocumentPreview(request);
    for (let generation = 2; generation < 40; generation += 1) {
      void cancelDocumentPreview({ ...request, generation, request_id: `cancel-${generation}` });
      void cancelDocumentPreview({ ...request, owner: "favorites", generation, request_id: `favorite-${generation}` });
    }
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    finish();
    await running;
    expect(bridge.invoke).toHaveBeenCalledTimes(3);
    expect(bridge.invoke.mock.calls.slice(1).map(([, value]) => value.request.generation)).toEqual([39, 39]);
  });
});
