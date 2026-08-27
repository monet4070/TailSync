export const PREVIEW_RESPONSE_MAGIC = "TSPV";
export const PREVIEW_RESPONSE_VERSION = 1;
export const PREVIEW_HEADER_BYTES = 9;
export const PREVIEW_MAX_BYTES = 64 * 1024 * 1024;
export const PREVIEW_MAX_METADATA_BYTES = 1024 * 1024;

export type PreviewKind = "image" | "text" | "file";

export type PreviewRenderer =
  | "image"
  | "svg"
  | "text"
  | "code"
  | "markdown"
  | "pdf"
  | "docx"
  | "unsupported";

export interface PreviewBatchNavigation {
  batch_id: string;
  item_index: number;
  item_count: number;
  first_entry_id: number;
  last_entry_id: number;
  previous_entry_id: number | null;
  next_entry_id: number | null;
}

export interface PreviewMetadata {
  entry_id?: number;
  kind: PreviewKind;
  name: string;
  size_bytes: number;
  width: number | null;
  height: number | null;
  batch: PreviewBatchNavigation | null;
}

export interface PreviewPayload extends PreviewMetadata {
  data: Uint8Array;
}

export type PreviewResponseMetadata = PreviewMetadata;

export type PreviewParseErrorCode =
  | "invalid_input"
  | "truncated_header"
  | "invalid_magic"
  | "unsupported_version"
  | "invalid_metadata_length"
  | "invalid_metadata_json"
  | "invalid_metadata"
  | "payload_too_large"
  | "invalid_image_dimensions"
  | "invalid_image_payload";

/** Raw IPC can surface as an ArrayBuffer or as a WebView2 byte array. */
export type PreviewResponseInput =
  | ArrayBufferLike
  | Uint8Array<ArrayBufferLike>
  | readonly number[];

export class PreviewParseError extends Error {
  readonly code: PreviewParseErrorCode;

  constructor(code: PreviewParseErrorCode, message: string) {
    super(message);
    this.name = "PreviewParseError";
    this.code = code;
  }
}
