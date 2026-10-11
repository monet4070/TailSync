import * as Contract from "../types/localContracts.generated";

export const DOCUMENT_RESPONSE_MAGIC = "TSDC";
export const DOCUMENT_RESPONSE_VERSION = 1;
export const DOCUMENT_HEADER_BYTES = 9;
export const DOCUMENT_MAX_METADATA_BYTES = 1024 * 1024;
export const DOCUMENT_MAX_MODEL_BYTES = 4 * 1024 * 1024;
export const DOCUMENT_MAX_BYTES = 16 * 1024 * 1024;

export type DocumentResponseInput =
  | ArrayBufferLike
  | Uint8Array<ArrayBufferLike>
  | readonly number[];

export type DocumentParseErrorCode =
  | "invalid_input"
  | "truncated_header"
  | "invalid_magic"
  | "unsupported_version"
  | "invalid_metadata_length"
  | "invalid_metadata_json"
  | "invalid_metadata"
  | "payload_too_large"
  | "invalid_assets";

export class DocumentParseError extends Error {
  readonly code: DocumentParseErrorCode;

  constructor(code: DocumentParseErrorCode, message: string) {
    super(message);
    this.name = "DocumentParseError";
    this.code = code;
  }
}

export type DecodedDocumentFrame = {
  metadata: Contract.DocumentFrameMetadata;
  document: Contract.DisplayDocument;
  assets: Uint8Array;
};

function failure(code: DocumentParseErrorCode, message: string): never {
  throw new DocumentParseError(code, message);
}

function inputBytes(input: DocumentResponseInput): Uint8Array {
  if (input instanceof Uint8Array) return input;
  if (Array.isArray(input)) {
    if (input.some((value) => !Number.isInteger(value) || value < 0 || value > 0xff)) {
      return failure("invalid_input", "Document response contains an invalid byte array");
    }
    return Uint8Array.from(input);
  }
  if (
    input instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer)
  ) {
    return new Uint8Array(input);
  }
  return failure("invalid_input", "Document response must be an ArrayBuffer, Uint8Array, or byte array");
}

function readUint32(bytes: Uint8Array, offset: number): number {
  return (
    bytes[offset] |
    (bytes[offset + 1] << 8) |
    (bytes[offset + 2] << 16) |
    (bytes[offset + 3] << 24)
  ) >>> 0;
}

function validateAssets(document: Contract.DisplayDocument, payloadLength: number): void {
  let expectedOffset = 0;
  for (const [position, asset] of document.assets.entries()) {
    if (asset.index !== position) {
      return failure("invalid_assets", "Document asset order is invalid");
    }
    if (asset.offset !== expectedOffset) {
      return failure("invalid_assets", "Document asset offsets are not contiguous");
    }
    if (!Number.isSafeInteger(asset.offset) || !Number.isSafeInteger(asset.length) || asset.length <= 0) {
      return failure("invalid_assets", "Document asset length is invalid");
    }
    expectedOffset = asset.offset + asset.length;
    if (expectedOffset > payloadLength) {
      return failure("invalid_assets", "Document asset exceeds the payload");
    }
  }
  if (expectedOffset !== payloadLength) {
    return failure("invalid_assets", "Document asset payload length does not match");
  }
}

export function decodeDocumentFrame(input: DocumentResponseInput, expected?: Contract.DocumentRequest): DecodedDocumentFrame {
  const bytes = inputBytes(input);
  if (bytes.length < DOCUMENT_HEADER_BYTES) {
    return failure("truncated_header", "Document frame header is truncated");
  }
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== DOCUMENT_RESPONSE_MAGIC) {
    return failure("invalid_magic", "Document frame magic is invalid");
  }
  if (bytes[4] !== DOCUMENT_RESPONSE_VERSION) {
    return failure("unsupported_version", "Document frame version is unsupported");
  }
  const metadataLength = readUint32(bytes, 5);
  if (metadataLength === 0 || metadataLength > DOCUMENT_MAX_METADATA_BYTES) {
    return failure("invalid_metadata_length", "Document metadata length is invalid");
  }
  const metadataEnd = DOCUMENT_HEADER_BYTES + metadataLength;
  if (bytes.length < metadataEnd) {
    return failure("truncated_header", "Document frame metadata is truncated");
  }
  if (bytes.length > DOCUMENT_MAX_BYTES) {
    return failure("payload_too_large", "Document frame exceeds the shared limit");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(DOCUMENT_HEADER_BYTES, metadataEnd)));
  } catch {
    return failure("invalid_metadata_json", "Document metadata is not valid JSON");
  }
  let metadata: Contract.DocumentFrameMetadata;
  try {
    metadata = Contract.decodeDocumentFrameMetadata(parsed);
  } catch {
    return failure("invalid_metadata", "Document metadata is invalid");
  }
  if (expected && (metadata.entry_id !== expected.entry_id || metadata.request_id !== expected.request_id || metadata.owner !== expected.owner || metadata.generation !== expected.generation)) {
    return failure("invalid_metadata", "Document request identity is stale");
  }
  const payload = bytes.subarray(metadataEnd);
  if (payload.length > DOCUMENT_MAX_BYTES) {
    return failure("payload_too_large", "Document payload exceeds the shared limit");
  }
  if (payload.length !== metadata.size_bytes) {
    return failure("invalid_assets", "Document payload length does not match the metadata");
  }
  let document: Contract.DisplayDocument;
  try {
    document = Contract.decodeDisplayDocument(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(payload.subarray(0, metadata.model_bytes))));
  } catch {
    return failure("invalid_metadata", "Document model is invalid");
  }
  const assets = payload.subarray(metadata.model_bytes);
  validateAssets(document, assets.length);
  return { metadata, document, assets };
}

function toHex(buffer: ArrayBuffer): string {
  return Array.from(new Uint8Array(buffer))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

export async function documentAssetBytes(
  frame: DecodedDocumentFrame,
  asset: Contract.DocumentAsset,
): Promise<Uint8Array<ArrayBuffer> | null> {
  if (asset.index >= frame.document.assets.length) return null;
  const declared = frame.document.assets[asset.index];
  if (declared.offset !== asset.offset || declared.length !== asset.length || declared.sha256 !== asset.sha256 || declared.width !== asset.width || declared.height !== asset.height) return null;
  const start = asset.offset;
  const length = asset.length;
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(length)) return null;
  if (start < 0 || length <= 0 || start + length > frame.assets.length) return null;
  const slice = frame.assets.subarray(start, start + length);
  if (slice.length !== length) return null;
  if (slice.length < 24 || ![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82].every((byte, index) => slice[index] === byte)) return null;
  const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
  if (view.getUint32(16) !== asset.width || view.getUint32(20) !== asset.height) return null;
  const owned = Uint8Array.from(slice);
  const digest = await crypto.subtle.digest("SHA-256", owned);
  if (toHex(digest) !== asset.sha256) return null;
  return owned;
}
