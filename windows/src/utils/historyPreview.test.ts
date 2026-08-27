import { describe, expect, it } from "vitest";
import {
  PREVIEW_HEADER_BYTES,
  PREVIEW_MAX_BYTES,
  PREVIEW_MAX_METADATA_BYTES,
  PREVIEW_RESPONSE_VERSION,
  PreviewParseError,
  type PreviewResponseInput,
  getPreviewMimeType,
  parsePreviewResponse,
  selectPreviewRenderer,
} from "./historyPreview";

type Metadata = {
  entry_id?: number;
  kind: "image" | "text" | "file";
  name: string;
  size_bytes: number;
  width: number | null;
  height: number | null;
};

function encodeResponse(metadata: Metadata, payload: Uint8Array): Uint8Array {
  const metadataBytes = new TextEncoder().encode(JSON.stringify(metadata));
  const response = new Uint8Array(PREVIEW_HEADER_BYTES + metadataBytes.length + payload.length);
  response.set(new TextEncoder().encode("TSPV"), 0);
  response[4] = PREVIEW_RESPONSE_VERSION;
  new DataView(response.buffer).setUint32(5, metadataBytes.length, true);
  response.set(metadataBytes, PREVIEW_HEADER_BYTES);
  response.set(payload, PREVIEW_HEADER_BYTES + metadataBytes.length);
  return response;
}

function textMetadata(overrides: Partial<Metadata> = {}): Metadata {
  return {
    kind: "text",
    name: "text.txt",
    size_bytes: 5,
    width: null,
    height: null,
    ...overrides,
  };
}

function expectParseError(input: PreviewResponseInput, code: PreviewParseError["code"]) {
  try {
    parsePreviewResponse(input);
    throw new Error("expected parser to reject the response");
  } catch (error) {
    expect(error).toBeInstanceOf(PreviewParseError);
    expect(error).toMatchObject({ code });
  }
}

describe("parsePreviewResponse", () => {
  it("accepts the plain byte array returned by WebView2 raw IPC", () => {
    const body = new TextEncoder().encode("runtime preview");
    const response = encodeResponse(
      textMetadata({ name: "runtime.txt", size_bytes: body.length }),
      body,
    );
    const parsed = parsePreviewResponse(Array.from(response));

    expect(parsed.name).toBe("runtime.txt");
    expect(new TextDecoder().decode(parsed.data)).toBe("runtime preview");
  });

  it("rejects malformed values in a WebView2 byte array", () => {
    const body = new TextEncoder().encode("runtime preview");
    const response = Array.from(encodeResponse(
      textMetadata({ name: "runtime.txt", size_bytes: body.length }),
      body,
    ));
    response[0] = 256;

    expectParseError(response, "invalid_input");
  });

  it("accepts the backend-resolved entry id for collapsed batches", () => {
    const response = encodeResponse(
      textMetadata({ entry_id: 42 }),
      new TextEncoder().encode("hello"),
    );
    expect(parsePreviewResponse(response).entry_id).toBe(42);
  });

  it("parses text and file payloads from ArrayBuffer and offset Uint8Array inputs", () => {
    const textResponse = encodeResponse(textMetadata(), new TextEncoder().encode("hello"));
    const parsedText = parsePreviewResponse(textResponse.buffer);
    expect(parsedText).toMatchObject({
      kind: "text",
      name: "text.txt",
      size_bytes: 5,
      width: null,
      height: null,
    });
    expect([...parsedText.data]).toEqual([...new TextEncoder().encode("hello")]);

    const filePayload = new Uint8Array([0, 1, 2, 255]);
    const fileResponse = encodeResponse(
      textMetadata({ kind: "file", name: "archive.bin", size_bytes: filePayload.length }),
      filePayload,
    );
    const padded = new Uint8Array(fileResponse.length + 4);
    padded.set(fileResponse, 2);
    const parsedFile = parsePreviewResponse(padded.subarray(2, 2 + fileResponse.length));
    expect(parsedFile.kind).toBe("file");
    expect(parsedFile.name).toBe("archive.bin");
    expect([...parsedFile.data]).toEqual([...filePayload]);
  });

  it("validates image dimensions against the exact raw RGBA payload length", () => {
    const rgba = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
    const response = encodeResponse(
      {
        kind: "image",
        name: "image",
        size_bytes: 12,
        width: 2,
        height: 1,
      },
      rgba,
    );
    const parsed = parsePreviewResponse(response);
    expect(parsed.width).toBe(2);
    expect(parsed.height).toBe(1);
    expect(parsed.data).toEqual(rgba);

    expectParseError(
      encodeResponse(
        {
          kind: "image",
          name: "image",
          size_bytes: 12,
          width: 3,
          height: 1,
        },
        rgba,
      ),
      "invalid_image_payload",
    );
    expectParseError(
      encodeResponse(
        {
          kind: "image",
          name: "image",
          size_bytes: 12,
          width: 0,
          height: 1,
        },
        rgba,
      ),
      "invalid_image_dimensions",
    );
  });

  it("rejects truncated, forged, and unsupported envelopes before rendering", () => {
    expectParseError(new Uint8Array(0), "truncated_header");
    expectParseError(new Uint8Array(PREVIEW_HEADER_BYTES - 1), "truncated_header");

    const wrongMagic = encodeResponse(textMetadata(), new TextEncoder().encode("hello"));
    wrongMagic[0] = "X".charCodeAt(0);
    expectParseError(wrongMagic, "invalid_magic");

    const wrongVersion = encodeResponse(textMetadata(), new TextEncoder().encode("hello"));
    wrongVersion[4] = PREVIEW_RESPONSE_VERSION + 1;
    expectParseError(wrongVersion, "unsupported_version");

    const forgedLength = encodeResponse(textMetadata(), new TextEncoder().encode("hello"));
    new DataView(forgedLength.buffer).setUint32(5, forgedLength.length, true);
    expectParseError(forgedLength, "invalid_metadata_length");

    const emptyMetadataLength = new Uint8Array(PREVIEW_HEADER_BYTES);
    emptyMetadataLength.set(new TextEncoder().encode("TSPV"));
    emptyMetadataLength[4] = PREVIEW_RESPONSE_VERSION;
    expectParseError(emptyMetadataLength, "invalid_metadata_length");
  });

  it("requires the exact v1 metadata schema and matching text/file sizes", () => {
    const valid = textMetadata();
    const malformed = [
      { ...valid, kind: "audio" as never },
      { ...valid, name: "" },
      { ...valid, size_bytes: -1 },
      { ...valid, size_bytes: 4.5 },
      { ...valid, width: 1 },
      { ...valid, extra: true } as Metadata,
    ];
    for (const metadata of malformed) {
      expectParseError(encodeResponse(metadata, new TextEncoder().encode("hello")), "invalid_metadata");
    }

    expectParseError(
      encodeResponse(textMetadata({ size_bytes: 4 }), new TextEncoder().encode("hello")),
      "invalid_metadata",
    );

    const invalidJson = encodeResponse(valid, new TextEncoder().encode("hello"));
    const metadataStart = PREVIEW_HEADER_BYTES;
    invalidJson[metadataStart] = 0xff;
    expectParseError(invalidJson, "invalid_metadata_json");
  });

  it("enforces metadata and payload size limits", () => {
    const oversizedMetadata = new Uint8Array(PREVIEW_HEADER_BYTES + PREVIEW_MAX_METADATA_BYTES + 1);
    oversizedMetadata.set(new TextEncoder().encode("TSPV"));
    oversizedMetadata[4] = PREVIEW_RESPONSE_VERSION;
    new DataView(oversizedMetadata.buffer).setUint32(5, PREVIEW_MAX_METADATA_BYTES + 1, true);
    expectParseError(oversizedMetadata, "invalid_metadata_length");

    // The payload itself is bounded independently of metadata.size_bytes.
    const oversizedPayload = new Uint8Array(PREVIEW_MAX_BYTES + 1);
    const metadata = textMetadata({ size_bytes: 1 });
    expectParseError(encodeResponse(metadata, oversizedPayload), "payload_too_large");
  });
});

describe("preview renderer and MIME policy", () => {
  it("selects all supported file renderers and gives SVG a visual renderer", () => {
    expect(selectPreviewRenderer("image", "image")).toBe("image");
    expect(selectPreviewRenderer("text", "text.txt")).toBe("text");
    expect(selectPreviewRenderer("file", "note.txt")).toBe("text");
    expect(selectPreviewRenderer("file", "README.MD")).toBe("markdown");
    expect(selectPreviewRenderer("file", "manual.pdf")).toBe("pdf");
    expect(selectPreviewRenderer("file", "letter.docx")).toBe("docx");
    expect(selectPreviewRenderer("file", "photo.png")).toBe("image");
    expect(selectPreviewRenderer("file", "photo.jpg")).toBe("image");
    expect(selectPreviewRenderer("file", "photo.jpeg")).toBe("image");
    expect(selectPreviewRenderer("file", "photo.gif")).toBe("image");
    expect(selectPreviewRenderer("file", "photo.webp")).toBe("image");
    expect(selectPreviewRenderer("file", "vector.svg")).toBe("svg");
    expect(selectPreviewRenderer("file", "archive.zip")).toBe("unsupported");
    expect(selectPreviewRenderer("file", "README")).toBe("unsupported");
  });

  it("maps MIME types without allowing SVG image execution", () => {
    expect(getPreviewMimeType("image", "image")).toBeNull();
    expect(getPreviewMimeType("text", "text.txt")).toBe("text/plain");
    expect(getPreviewMimeType("file", "note.txt")).toBe("text/plain");
    expect(getPreviewMimeType("file", "README.md")).toBe("text/markdown");
    expect(getPreviewMimeType("file", "manual.pdf")).toBe("application/pdf");
    expect(getPreviewMimeType("file", "letter.docx")).toBe(
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
    expect(getPreviewMimeType("file", "photo.png")).toBe("image/png");
    expect(getPreviewMimeType("file", "photo.jpg")).toBe("image/jpeg");
    expect(getPreviewMimeType("file", "photo.jpeg")).toBe("image/jpeg");
    expect(getPreviewMimeType("file", "photo.gif")).toBe("image/gif");
    expect(getPreviewMimeType("file", "photo.webp")).toBe("image/webp");
    expect(getPreviewMimeType("file", "vector.svg")).toBeNull();
    expect(getPreviewMimeType("file", "archive.zip")).toBeNull();
  });
});
