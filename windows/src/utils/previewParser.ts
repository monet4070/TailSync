import {
  PREVIEW_HEADER_BYTES,
  PREVIEW_MAX_BYTES,
  PREVIEW_MAX_METADATA_BYTES,
  PREVIEW_RESPONSE_MAGIC,
  PREVIEW_RESPONSE_VERSION,
  PreviewParseError,
  type PreviewBatchNavigation,
  type PreviewMetadata,
  type PreviewParseErrorCode,
  type PreviewPayload,
  type PreviewResponseInput,
} from "./previewTypes";

const LEGACY_METADATA_KEYS = ["height", "kind", "name", "size_bytes", "width"].sort();
const ENTRY_METADATA_KEYS = ["entry_id", ...LEGACY_METADATA_KEYS].sort();
const CURRENT_METADATA_KEYS = ["batch", ...ENTRY_METADATA_KEYS].sort();
const BATCH_METADATA_KEYS = [
  "batch_id",
  "first_entry_id",
  "item_count",
  "item_index",
  "last_entry_id",
  "next_entry_id",
  "previous_entry_id",
].sort();
const UINT32_MAX = 0xffff_ffff;

function parseFailure(code: PreviewParseErrorCode, message: string): never {
  throw new PreviewParseError(code, message);
}

function inputBytes(input: PreviewResponseInput): Uint8Array {
  if (input instanceof Uint8Array) return input;
  if (Array.isArray(input)) {
    if (input.some((value) => !Number.isInteger(value) || value < 0 || value > 0xff)) {
      return parseFailure("invalid_input", "Preview response contains an invalid byte array");
    }
    return Uint8Array.from(input);
  }
  if (
    input instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer)
  ) {
    return new Uint8Array(input);
  }
  return parseFailure(
    "invalid_input",
    "Preview response must be an ArrayBuffer, Uint8Array, or byte array",
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactMetadataKeys(value: Record<string, unknown>): boolean {
  const keys = Object.keys(value).sort();
  return [LEGACY_METADATA_KEYS, ENTRY_METADATA_KEYS, CURRENT_METADATA_KEYS].some(
    (knownKeys) =>
      keys.length === knownKeys.length &&
      keys.every((key, index) => key === knownKeys[index]),
  );
}

function isPositiveEntryId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function readBatchNavigation(value: unknown): PreviewBatchNavigation | null {
  if (value === undefined || value === null) return null;
  if (!isRecord(value)) {
    return parseFailure("invalid_metadata", "Preview batch metadata is invalid");
  }
  const keys = Object.keys(value).sort();
  if (
    keys.length !== BATCH_METADATA_KEYS.length ||
    !keys.every((key, index) => key === BATCH_METADATA_KEYS[index])
  ) {
    return parseFailure("invalid_metadata", "Preview batch metadata fields are invalid");
  }
  const {
    batch_id: batchId,
    item_index: itemIndex,
    item_count: itemCount,
    first_entry_id: firstEntryId,
    last_entry_id: lastEntryId,
    previous_entry_id: previousEntryId,
    next_entry_id: nextEntryId,
  } = value;
  if (
    typeof batchId !== "string" ||
    batchId.length === 0 ||
    batchId.length > 256 ||
    !Number.isSafeInteger(itemIndex) ||
    (itemIndex as number) < 0 ||
    !Number.isSafeInteger(itemCount) ||
    (itemCount as number) <= 0 ||
    (itemIndex as number) >= (itemCount as number) ||
    !isPositiveEntryId(firstEntryId) ||
    !isPositiveEntryId(lastEntryId) ||
    (previousEntryId !== null && !isPositiveEntryId(previousEntryId)) ||
    (nextEntryId !== null && !isPositiveEntryId(nextEntryId))
  ) {
    return parseFailure("invalid_metadata", "Preview batch metadata values are invalid");
  }
  return {
    batch_id: batchId,
    item_index: itemIndex as number,
    item_count: itemCount as number,
    first_entry_id: firstEntryId,
    last_entry_id: lastEntryId,
    previous_entry_id: previousEntryId as number | null,
    next_entry_id: nextEntryId as number | null,
  };
}

function isSafeUint32(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= UINT32_MAX
  );
}

function readMetadata(metadataBytes: Uint8Array): PreviewMetadata {
  let decoded: unknown;
  try {
    decoded = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(metadataBytes));
  } catch {
    return parseFailure("invalid_metadata_json", "Preview metadata is not valid UTF-8 JSON");
  }
  if (!isRecord(decoded) || !hasExactMetadataKeys(decoded)) {
    return parseFailure("invalid_metadata", "Preview metadata fields are invalid");
  }

  const {
    entry_id: entryId,
    kind,
    name,
    size_bytes: sizeBytes,
    width,
    height,
    batch,
  } = decoded;
  if (
    entryId !== undefined &&
    (typeof entryId !== "number" || !Number.isSafeInteger(entryId) || entryId < 0)
  ) {
    return parseFailure("invalid_metadata", "Preview metadata entry_id is invalid");
  }
  if (kind !== "image" && kind !== "text" && kind !== "file") {
    return parseFailure("invalid_metadata", "Preview metadata has an unsupported kind");
  }
  if (typeof name !== "string" || name.length === 0) {
    return parseFailure("invalid_metadata", "Preview metadata name must be a non-empty string");
  }
  if (
    typeof sizeBytes !== "number" ||
    !Number.isSafeInteger(sizeBytes) ||
    sizeBytes < 0 ||
    sizeBytes > PREVIEW_MAX_BYTES
  ) {
    return parseFailure("invalid_metadata", "Preview metadata size_bytes is invalid");
  }

  if (kind === "image") {
    if (!isSafeUint32(width) || width === 0 || !isSafeUint32(height) || height === 0) {
      return parseFailure("invalid_image_dimensions", "Image preview dimensions are invalid");
    }
    return {
      ...(entryId === undefined ? {} : { entry_id: entryId }),
      kind,
      name,
      size_bytes: sizeBytes,
      width,
      height,
      batch: readBatchNavigation(batch),
    };
  }

  if (width !== null || height !== null) {
    return parseFailure(
      "invalid_metadata",
      "Only image previews may contain width and height",
    );
  }
  return {
    ...(entryId === undefined ? {} : { entry_id: entryId }),
    kind,
    name,
    size_bytes: sizeBytes,
    width: null,
    height: null,
    batch: readBatchNavigation(batch),
  };
}

/** Parse and validate one versioned TSPV IPC envelope. */
export function parsePreviewResponse(input: PreviewResponseInput): PreviewPayload {
  const bytes = inputBytes(input);
  if (bytes.length < PREVIEW_HEADER_BYTES) {
    return parseFailure("truncated_header", "Preview response header is truncated");
  }

  for (let index = 0; index < PREVIEW_RESPONSE_MAGIC.length; index += 1) {
    if (bytes[index] !== PREVIEW_RESPONSE_MAGIC.charCodeAt(index)) {
      return parseFailure("invalid_magic", "Preview response magic is invalid");
    }
  }
  if (bytes[4] !== PREVIEW_RESPONSE_VERSION) {
    return parseFailure(
      "unsupported_version",
      `Preview response version ${bytes[4]} is unsupported`,
    );
  }

  const metadataLength =
    bytes[5] |
    (bytes[6] << 8) |
    (bytes[7] << 16) |
    (bytes[8] << 24);
  const metadataBytesLength = metadataLength >>> 0;
  if (
    metadataBytesLength === 0 ||
    metadataBytesLength > PREVIEW_MAX_METADATA_BYTES ||
    metadataBytesLength > bytes.length - PREVIEW_HEADER_BYTES
  ) {
    return parseFailure("invalid_metadata_length", "Preview metadata length is out of bounds");
  }

  const metadataEnd = PREVIEW_HEADER_BYTES + metadataBytesLength;
  const metadata = readMetadata(bytes.slice(PREVIEW_HEADER_BYTES, metadataEnd));
  const payloadLength = bytes.length - metadataEnd;
  if (payloadLength > PREVIEW_MAX_BYTES) {
    return parseFailure(
      "payload_too_large",
      `Preview payload exceeds the ${PREVIEW_MAX_BYTES}-byte limit`,
    );
  }
  const payload = bytes.slice(metadataEnd);

  if (metadata.kind === "image") {
    const expectedLength = metadata.width! * metadata.height! * 4;
    if (!Number.isSafeInteger(expectedLength)) {
      return parseFailure(
        "invalid_image_dimensions",
        "Image preview dimensions overflow the supported range",
      );
    }
    if (expectedLength !== payload.length) {
      return parseFailure(
        "invalid_image_payload",
        `Image payload length ${payload.length} does not match ${metadata.width}x${metadata.height} RGBA bytes`,
      );
    }
  } else if (metadata.size_bytes !== payload.length) {
    return parseFailure(
      "invalid_metadata",
      "Preview size_bytes does not match the payload length",
    );
  }

  return { ...metadata, data: payload };
}
