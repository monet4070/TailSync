import { createHash } from "node:crypto";
import { deflateSync } from "node:zlib";


export interface DocumentFrameControl {
  entry_id: number;
  owner: string;
  generation: number;
  request_id: string;
  name?: string;
  stamp?: string;
}

export interface BuiltDocumentAsset {
  asset: Record<string, unknown>;
  bytes: Uint8Array;
  width: number;
  height: number;
}

const PNG_SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82];

export function buildPngAsset(width: number, height: number, fill: number): Uint8Array {
  const checksum = (bytes: Uint8Array): number => {
    let result = 0xffffffff;
    for (const byte of bytes) {
      result ^= byte;
      for (let bit = 0; bit < 8; bit += 1) result = (result >>> 1) ^ ((result & 1) ? 0xedb88320 : 0);
    }
    return (result ^ 0xffffffff) >>> 0;
  };
  const chunk = (type: string, data: Uint8Array): Buffer => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const result = Buffer.alloc(data.length + 12);
    result.writeUInt32BE(data.length, 0);
    body.copy(result, 4);
    result.writeUInt32BE(checksum(body), result.length - 4);
    return result;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 6;
  const pixels = Buffer.alloc((width * 4 + 1) * height, fill);
  for (let row = 0; row < height; row += 1) pixels[row * (width * 4 + 1)] = 0;
  return Buffer.concat([Buffer.from(PNG_SIGNATURE.slice(0, 8)), chunk("IHDR", header), chunk("IDAT", deflateSync(pixels)), chunk("IEND", new Uint8Array())]);
}

export function documentAsset(index: number, offset: number, source: Uint8Array): BuiltDocumentAsset {
  const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
  return {
    asset: {
      index,
      offset,
      length: source.byteLength,
      width: view.getUint32(16, false),
      height: view.getUint32(20, false),
      sha256: createHash("sha256").update(source).digest("hex"),
    },
    bytes: source,
    width: view.getUint32(16, false),
    height: view.getUint32(20, false),
  };
}

export function encodeDocumentFrame(
  control: DocumentFrameControl,
  document: Record<string, unknown>,
  payload: Uint8Array,
): Uint8Array {
  const model = new TextEncoder().encode(JSON.stringify(document));
  const metadata = new TextEncoder().encode(JSON.stringify({
    request_id: control.request_id,
    entry_id: control.entry_id,
    kind: "document",
    name: control.name ?? "document",
    stamp: control.stamp ?? `dc1:${"0a".repeat(32)}`,
    owner: control.owner,
    generation: control.generation,
    model_bytes: model.length,
    size_bytes: model.length + payload.length,
    batch: null,
  }));
  const frame = new Uint8Array(9 + metadata.length + model.length + payload.length);
  frame.set(new TextEncoder().encode("TSDC"), 0);
  frame[4] = 1;
  new DataView(frame.buffer).setUint32(5, metadata.length, true);
  frame.set(metadata, 9);
  frame.set(model, 9 + metadata.length);
  frame.set(payload, 9 + metadata.length + model.length);
  return frame;
}

export function documentRunStyle(changes: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    bold: false,
    italic: false,
    underline: false,
    strikethrough: false,
    subscript: false,
    superscript: false,
    ...changes,
  };
}

export function documentRun(text: string, style: Record<string, unknown> = {}): Record<string, unknown> {
  return { text, style: documentRunStyle(style), unsupported: false };
}

export function documentParagraph(
  runs: Array<Record<string, unknown>>,
  list: string | null = null,
): Record<string, unknown> {
  return { kind: "paragraph", paragraph: list ? { list, runs } : { runs } };
}

export function documentImageBlock(asset: number | null, ordinal: number, availability = "available"): Record<string, unknown> {
  return {
    kind: "image",
    image: asset === null ? { availability, ordinal } : { asset, availability, ordinal },
  };
}

export function documentTableBlock(rows: Array<Array<{ header: boolean; runs: Array<Record<string, unknown>> }>>, truncated = false): Record<string, unknown> {
  return {
    kind: "table",
    table: { rows: rows.map((cells) => ({ cells })), truncated },
  };
}

export function concatAssets(assets: BuiltDocumentAsset[]): Uint8Array {
  const total = assets.reduce((sum, entry) => sum + entry.bytes.byteLength, 0);
  const payload = new Uint8Array(total);
  let offset = 0;
  for (const entry of assets) {
    payload.set(entry.bytes, offset);
    offset += entry.bytes.byteLength;
  }
  return payload;
}
