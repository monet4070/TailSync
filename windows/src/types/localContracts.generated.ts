// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.
const isRecord = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);

function hasLoneSurrogate(value: string) {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xD800 && code <= 0xDBFF) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xDC00 && next <= 0xDFFF)) return true;
      index += 1;
    } else if (code >= 0xDC00 && code <= 0xDFFF) return true;
  }
  return false;
}
function validDocumentLink(value: string) {
  if (!value || new TextEncoder().encode(value).length > 2048 || value.includes("\\") || hasLoneSurrogate(value)) return false;
  const whiteSpace = /\p{White_Space}/u;
  for (const character of value) {
    const scalar = character.codePointAt(0) ?? 0;
    if (whiteSpace.test(character) || scalar <= 31 || (scalar >= 127 && scalar <= 159)) return false;
  }
  const authority = value.match(/^https?:\/\/([^/?#]*)/i)?.[1];
  if (!authority || authority.includes("@")) return false;
  try { const parsed = new URL(value); return (parsed.protocol === "http:" || parsed.protocol === "https:") && !!parsed.hostname && !parsed.username && !parsed.password; } catch { return false; }
}
function validDocumentSemantics(value: DisplayDocument) {
  let textBytes = 0, runs = 0, cells = 0, images = 0, offset = 0, pixels = 0;
  let first: number | null = null;
  const used = new Set<number>();
  const validRuns = (items: Array<DocumentRun>) => {
    runs += items.length;
    for (const run of items) {
      if (run.text.includes("\0") || hasLoneSurrogate(run.text)) return false;
      textBytes += new TextEncoder().encode(run.text).length;
      if (run.link != null) { if (!validDocumentLink(run.link)) return false; textBytes += new TextEncoder().encode(run.link).length; }
    }
    return runs <= 8192 && textBytes <= 1048576;
  };
  if (value.blocks.length > 8192 || value.assets.length > 64 || value.partial.length > 8192) return false;
  for (const block of value.blocks) {
    if (block.kind === "paragraph") {
      if (!block.paragraph || block.table || block.image || !validRuns(block.paragraph.runs)) return false;
      if (block.paragraph.list == null && (block.paragraph.list_depth ?? 0) !== 0) return false;
    }
    else if (block.kind === "table") {
      if (!block.table || block.paragraph || block.image || block.table.rows.length > 2048) return false;
      for (const row of block.table.rows) { if (!row.cells.length || row.cells.length > 2048) return false; for (const cell of row.cells) { cells++; if (cells > 2048 || !validRuns(cell.runs)) return false; } }
    } else {
      if (!block.image || block.paragraph || block.table || block.image.ordinal !== images || (block.image.availability === "available") !== (block.image.asset != null)) return false;
      if (images++ === 0) first = block.image.asset ?? null;
      if (block.image.asset != null) { if (block.image.asset >= value.assets.length) return false; used.add(block.image.asset); }
    }
  }
  if (images > 64 || (value.first_image ?? null) !== first || used.size !== value.assets.length) return false;
  for (const [index, asset] of value.assets.entries()) {
    if (asset.index !== index || asset.offset !== offset || asset.length <= 0 || asset.width <= 0 || asset.height <= 0 || !/^[0-9a-fA-F]{64}$/.test(asset.sha256)) return false;
    offset += asset.length; pixels += asset.width * asset.height;
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(pixels) || offset > 16777216 || pixels > 16000000) return false;
  }
  return value.partial.every(reason => !reason.detail.includes("\0") && !hasLoneSurrogate(reason.detail) && new TextEncoder().encode(reason.detail).length <= 512);
}
function validDocumentFrameSemantics(value: DocumentFrameMetadata) {
  return typeof value.request_id === "string" && value.request_id.length > 0 && !hasLoneSurrogate(value.request_id) && new TextEncoder().encode(value.request_id).length <= 128 && value.name.length > 0 && !hasLoneSurrogate(value.name) && new TextEncoder().encode(value.name).length <= 256 && /^dc1:[0-9a-fA-F]{64}$/.test(value.stamp) && value.model_bytes > 0 && value.model_bytes <= value.size_bytes && value.batch == null;
}
export const DOCUMENT_MAX_METADATA_BYTES = 1048576;
export const DOCUMENT_MAX_MODEL_BYTES = 4194304;
export const DOCUMENT_MAX_BYTES = 16777216;

export type ActiveRoute = {
  "address": string;
  "interface": ConnectionInterface;
  "latency": number;
};
function validActiveRoute(value: unknown): value is ActiveRoute { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (Object.hasOwn(value, "latency") && (typeof value["latency"] === "number" && Number.isSafeInteger(value["latency"]) && value["latency"] >= 0))); }
export function decodeActiveRoute(value: unknown): ActiveRoute { if (!validActiveRoute(value)) throw new Error("Invalid ActiveRoute response"); return value; }

export type ArchiveEntryImagesReport = {
  "archived": number;
  "complete": boolean;
  "deduplicated": number;
  "failures": Array<ArchiveResourceFailure>;
};
function validArchiveEntryImagesReport(value: unknown): value is ArchiveEntryImagesReport { return (isRecord(value) && (Object.hasOwn(value, "archived") && (typeof value["archived"] === "number" && Number.isSafeInteger(value["archived"]) && value["archived"] >= 0 && value["archived"] <= 65535)) && (Object.hasOwn(value, "complete") && (typeof value["complete"] === "boolean")) && (Object.hasOwn(value, "deduplicated") && (typeof value["deduplicated"] === "number" && Number.isSafeInteger(value["deduplicated"]) && value["deduplicated"] >= 0 && value["deduplicated"] <= 65535)) && (Object.hasOwn(value, "failures") && (Array.isArray(value["failures"]) && value["failures"].every(item => validArchiveResourceFailure(item))))); }
export function decodeArchiveEntryImagesReport(value: unknown): ArchiveEntryImagesReport { if (!validArchiveEntryImagesReport(value)) throw new Error("Invalid ArchiveEntryImagesReport response"); return value; }

export type ArchiveResourceFailure = {
  "code": string;
  "image_index": number;
};
function validArchiveResourceFailure(value: unknown): value is ArchiveResourceFailure { return (isRecord(value) && (Object.hasOwn(value, "code") && (typeof value["code"] === "string")) && (Object.hasOwn(value, "image_index") && (typeof value["image_index"] === "number" && Number.isSafeInteger(value["image_index"]) && value["image_index"] >= 0 && value["image_index"] <= 65535))); }
export function decodeArchiveResourceFailure(value: unknown): ArchiveResourceFailure { if (!validArchiveResourceFailure(value)) throw new Error("Invalid ArchiveResourceFailure response"); return value; }

export type ClipboardArrival = {
  "count": number;
  "entry_id": number | null;
  "event_id": string;
  "kind": string;
  "source_id": string;
  "source_name": string;
};
function validClipboardArrival(value: unknown): value is ClipboardArrival { return (isRecord(value) && (Object.hasOwn(value, "count") && (typeof value["count"] === "number" && Number.isSafeInteger(value["count"]) && value["count"] >= 0)) && (Object.hasOwn(value, "entry_id") && ((typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"])) || (value["entry_id"] === null))) && (Object.hasOwn(value, "event_id") && (typeof value["event_id"] === "string")) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string")) && (Object.hasOwn(value, "source_id") && (typeof value["source_id"] === "string")) && (Object.hasOwn(value, "source_name") && (typeof value["source_name"] === "string"))); }
export function decodeClipboardArrival(value: unknown): ClipboardArrival { if (!validClipboardArrival(value)) throw new Error("Invalid ClipboardArrival response"); return value; }

export type CompositeInfo = {
  "completeness": "complete" | "external_references" | "missing_formats";
  "formats": Array<string>;
  "identity"?: HistoryIdentity | null;
  "image_count": number;
  "native_item_count": number;
  "native_only": boolean;
  "representation_count": number;
  "restorable_formats": Array<string>;
  "withheld_formats": Array<WithheldRestoreFormat>;
};
function validCompositeInfo(value: unknown): value is CompositeInfo { return (isRecord(value) && (Object.hasOwn(value, "completeness") && (typeof value["completeness"] === "string" && (value["completeness"] === "complete" || value["completeness"] === "external_references" || value["completeness"] === "missing_formats"))) && (Object.hasOwn(value, "formats") && (Array.isArray(value["formats"]) && value["formats"].every(item => (typeof item === "string")))) && (!Object.hasOwn(value, "identity") || (validHistoryIdentity(value["identity"]) || (value["identity"] === null))) && (Object.hasOwn(value, "image_count") && (typeof value["image_count"] === "number" && Number.isSafeInteger(value["image_count"]) && value["image_count"] >= 0 && value["image_count"] <= 65535)) && (Object.hasOwn(value, "native_item_count") && (typeof value["native_item_count"] === "number" && Number.isSafeInteger(value["native_item_count"]) && value["native_item_count"] >= 0 && value["native_item_count"] <= 65535)) && (Object.hasOwn(value, "native_only") && (typeof value["native_only"] === "boolean")) && (Object.hasOwn(value, "representation_count") && (typeof value["representation_count"] === "number" && Number.isSafeInteger(value["representation_count"]) && value["representation_count"] >= 0 && value["representation_count"] <= 65535)) && (Object.hasOwn(value, "restorable_formats") && (Array.isArray(value["restorable_formats"]) && value["restorable_formats"].every(item => (typeof item === "string")))) && (Object.hasOwn(value, "withheld_formats") && (Array.isArray(value["withheld_formats"]) && value["withheld_formats"].every(item => validWithheldRestoreFormat(item))))); }
export function decodeCompositeInfo(value: unknown): CompositeInfo { if (!validCompositeInfo(value)) throw new Error("Invalid CompositeInfo response"); return value; }

export type ConnectionInterface = "lan" | "iroh" | "tailscale";
function validConnectionInterface(value: unknown): value is ConnectionInterface { return (typeof value === "string" && (value === "lan" || value === "iroh" || value === "tailscale")); }
export function decodeConnectionInterface(value: unknown): ConnectionInterface { if (!validConnectionInterface(value)) throw new Error("Invalid ConnectionInterface response"); return value; }

export type DaemonStatus = {
  "active_routes": Record<string, ActiveRoute>;
  "clipboard_monitor_failures": number;
  "clipboard_monitor_healthy": boolean;
  "tcp_server_healthy": boolean;
};
function validDaemonStatus(value: unknown): value is DaemonStatus { return (isRecord(value) && (Object.hasOwn(value, "active_routes") && (isRecord(value["active_routes"]) && Object.values(value["active_routes"]).every(item => validActiveRoute(item)))) && (Object.hasOwn(value, "clipboard_monitor_failures") && (typeof value["clipboard_monitor_failures"] === "number" && Number.isSafeInteger(value["clipboard_monitor_failures"]) && value["clipboard_monitor_failures"] >= 0)) && (Object.hasOwn(value, "clipboard_monitor_healthy") && (typeof value["clipboard_monitor_healthy"] === "boolean")) && (Object.hasOwn(value, "tcp_server_healthy") && (typeof value["tcp_server_healthy"] === "boolean"))); }
export function decodeDaemonStatus(value: unknown): DaemonStatus { if (!validDaemonStatus(value)) throw new Error("Invalid DaemonStatus response"); return value; }

export type DisplayDocument = {
  "assets": Array<DocumentAsset>;
  "blocks": Array<DocumentBlock>;
  "first_image"?: number | null;
  "partial": Array<DocumentPartial>;
  "styles_partial": boolean;
  "version": number;
};
function validDisplayDocument(value: unknown): value is DisplayDocument { return (isRecord(value) && (Object.hasOwn(value, "assets") && (Array.isArray(value["assets"]) && value["assets"].every(item => validDocumentAsset(item)))) && (Object.hasOwn(value, "blocks") && (Array.isArray(value["blocks"]) && value["blocks"].every(item => validDocumentBlock(item)))) && (!Object.hasOwn(value, "first_image") || ((typeof value["first_image"] === "number" && Number.isSafeInteger(value["first_image"]) && value["first_image"] >= 0 && value["first_image"] <= 65535) || (value["first_image"] === null))) && (Object.hasOwn(value, "partial") && (Array.isArray(value["partial"]) && value["partial"].every(item => validDocumentPartial(item)))) && (Object.hasOwn(value, "styles_partial") && (typeof value["styles_partial"] === "boolean")) && (Object.hasOwn(value, "version") && (typeof value["version"] === "number" && Number.isSafeInteger(value["version"]) && value["version"] === 1))) && validDocumentSemantics(value as DisplayDocument); }
export function decodeDisplayDocument(value: unknown): DisplayDocument { if (!validDisplayDocument(value)) throw new Error("Invalid DisplayDocument response"); return value; }

export type DocumentAsset = {
  "height": number;
  "index": number;
  "length": number;
  "offset": number;
  "sha256": string;
  "width": number;
};
function validDocumentAsset(value: unknown): value is DocumentAsset { return (isRecord(value) && (Object.hasOwn(value, "height") && (typeof value["height"] === "number" && Number.isSafeInteger(value["height"]) && value["height"] >= 0)) && (Object.hasOwn(value, "index") && (typeof value["index"] === "number" && Number.isSafeInteger(value["index"]) && value["index"] >= 0 && value["index"] <= 65535)) && (Object.hasOwn(value, "length") && (typeof value["length"] === "number" && Number.isSafeInteger(value["length"]) && value["length"] >= 0)) && (Object.hasOwn(value, "offset") && (typeof value["offset"] === "number" && Number.isSafeInteger(value["offset"]) && value["offset"] >= 0)) && (Object.hasOwn(value, "sha256") && (typeof value["sha256"] === "string")) && (Object.hasOwn(value, "width") && (typeof value["width"] === "number" && Number.isSafeInteger(value["width"]) && value["width"] >= 0))); }
export function decodeDocumentAsset(value: unknown): DocumentAsset { if (!validDocumentAsset(value)) throw new Error("Invalid DocumentAsset response"); return value; }

export type DocumentBlock = {
  "image"?: DocumentImage | null;
  "kind": DocumentBlockKind;
  "paragraph"?: DocumentParagraph | null;
  "table"?: DocumentTable | null;
};
function validDocumentBlock(value: unknown): value is DocumentBlock { return (isRecord(value) && (!Object.hasOwn(value, "image") || (validDocumentImage(value["image"]) || (value["image"] === null))) && (Object.hasOwn(value, "kind") && validDocumentBlockKind(value["kind"])) && (!Object.hasOwn(value, "paragraph") || (validDocumentParagraph(value["paragraph"]) || (value["paragraph"] === null))) && (!Object.hasOwn(value, "table") || (validDocumentTable(value["table"]) || (value["table"] === null)))); }
export function decodeDocumentBlock(value: unknown): DocumentBlock { if (!validDocumentBlock(value)) throw new Error("Invalid DocumentBlock response"); return value; }

export type DocumentBlockKind = "paragraph" | "table" | "image";
function validDocumentBlockKind(value: unknown): value is DocumentBlockKind { return (typeof value === "string" && (value === "paragraph" || value === "table" || value === "image")); }
export function decodeDocumentBlockKind(value: unknown): DocumentBlockKind { if (!validDocumentBlockKind(value)) throw new Error("Invalid DocumentBlockKind response"); return value; }

export type DocumentColor = {
  "blue": number;
  "green": number;
  "red": number;
};
function validDocumentColor(value: unknown): value is DocumentColor { return (isRecord(value) && (Object.hasOwn(value, "blue") && (typeof value["blue"] === "number" && Number.isSafeInteger(value["blue"]) && value["blue"] >= 0 && value["blue"] <= 255)) && (Object.hasOwn(value, "green") && (typeof value["green"] === "number" && Number.isSafeInteger(value["green"]) && value["green"] >= 0 && value["green"] <= 255)) && (Object.hasOwn(value, "red") && (typeof value["red"] === "number" && Number.isSafeInteger(value["red"]) && value["red"] >= 0 && value["red"] <= 255))); }
export function decodeDocumentColor(value: unknown): DocumentColor { if (!validDocumentColor(value)) throw new Error("Invalid DocumentColor response"); return value; }

export type DocumentFrameMetadata = {
  "batch"?: PreviewBatchNavigation | null;
  "entry_id": number;
  "generation": number;
  "kind": "document";
  "model_bytes": number;
  "name": string;
  "owner": "history" | "favorites";
  "request_id"?: string | null;
  "size_bytes": number;
  "stamp": string;
};
function validDocumentFrameMetadata(value: unknown): value is DocumentFrameMetadata { return (isRecord(value) && (!Object.hasOwn(value, "batch") || (validPreviewBatchNavigation(value["batch"]) || (value["batch"] === null))) && (Object.hasOwn(value, "entry_id") && (typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"]) && value["entry_id"] >= 1)) && (Object.hasOwn(value, "generation") && (typeof value["generation"] === "number" && Number.isSafeInteger(value["generation"]) && value["generation"] >= 0 && value["generation"] <= 9007199254740991)) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string" && (value["kind"] === "document"))) && (Object.hasOwn(value, "model_bytes") && (typeof value["model_bytes"] === "number" && Number.isSafeInteger(value["model_bytes"]) && value["model_bytes"] >= 0 && value["model_bytes"] <= 4194304)) && (Object.hasOwn(value, "name") && (typeof value["name"] === "string" && value["name"].length >= 1)) && (Object.hasOwn(value, "owner") && (typeof value["owner"] === "string" && (value["owner"] === "history" || value["owner"] === "favorites"))) && (!Object.hasOwn(value, "request_id") || ((typeof value["request_id"] === "string") || (value["request_id"] === null))) && (Object.hasOwn(value, "size_bytes") && (typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]) && value["size_bytes"] >= 0 && value["size_bytes"] <= 16777216)) && (Object.hasOwn(value, "stamp") && (typeof value["stamp"] === "string" && value["stamp"].length >= 1))) && validDocumentFrameSemantics(value as DocumentFrameMetadata); }
export function decodeDocumentFrameMetadata(value: unknown): DocumentFrameMetadata { if (!validDocumentFrameMetadata(value)) throw new Error("Invalid DocumentFrameMetadata response"); return value; }

export type DocumentImage = {
  "asset"?: number | null;
  "availability": DocumentImageAvailability;
  "ordinal": number;
};
function validDocumentImage(value: unknown): value is DocumentImage { return (isRecord(value) && (!Object.hasOwn(value, "asset") || ((typeof value["asset"] === "number" && Number.isSafeInteger(value["asset"]) && value["asset"] >= 0 && value["asset"] <= 65535) || (value["asset"] === null))) && (Object.hasOwn(value, "availability") && validDocumentImageAvailability(value["availability"])) && (Object.hasOwn(value, "ordinal") && (typeof value["ordinal"] === "number" && Number.isSafeInteger(value["ordinal"]) && value["ordinal"] >= 0 && value["ordinal"] <= 65535))); }
export function decodeDocumentImage(value: unknown): DocumentImage { if (!validDocumentImage(value)) throw new Error("Invalid DocumentImage response"); return value; }

export type DocumentImageAvailability = "available" | "missing_resource" | "missing_source" | "external_reference" | "unsupported" | "pixel_budget";
function validDocumentImageAvailability(value: unknown): value is DocumentImageAvailability { return (typeof value === "string" && (value === "available" || value === "missing_resource" || value === "missing_source" || value === "external_reference" || value === "unsupported" || value === "pixel_budget")); }
export function decodeDocumentImageAvailability(value: unknown): DocumentImageAvailability { if (!validDocumentImageAvailability(value)) throw new Error("Invalid DocumentImageAvailability response"); return value; }

export type DocumentListKind = "bullet" | "ordered";
function validDocumentListKind(value: unknown): value is DocumentListKind { return (typeof value === "string" && (value === "bullet" || value === "ordered")); }
export function decodeDocumentListKind(value: unknown): DocumentListKind { if (!validDocumentListKind(value)) throw new Error("Invalid DocumentListKind response"); return value; }

export type DocumentParagraph = {
  "code_block"?: boolean;
  "heading"?: number | null;
  "list"?: DocumentListKind | null;
  "list_depth"?: number;
  "quote_depth"?: number;
  "runs": Array<DocumentRun>;
};
function validDocumentParagraph(value: unknown): value is DocumentParagraph { return (isRecord(value) && (!Object.hasOwn(value, "code_block") || (typeof value["code_block"] === "boolean")) && (!Object.hasOwn(value, "heading") || ((typeof value["heading"] === "number" && Number.isSafeInteger(value["heading"]) && value["heading"] >= 1 && value["heading"] <= 3) || (value["heading"] === null))) && (!Object.hasOwn(value, "list") || (validDocumentListKind(value["list"]) || (value["list"] === null))) && (!Object.hasOwn(value, "list_depth") || (typeof value["list_depth"] === "number" && Number.isSafeInteger(value["list_depth"]) && value["list_depth"] >= 0 && value["list_depth"] <= 8)) && (!Object.hasOwn(value, "quote_depth") || (typeof value["quote_depth"] === "number" && Number.isSafeInteger(value["quote_depth"]) && value["quote_depth"] >= 0 && value["quote_depth"] <= 8)) && (Object.hasOwn(value, "runs") && (Array.isArray(value["runs"]) && value["runs"].every(item => validDocumentRun(item))))); }
export function decodeDocumentParagraph(value: unknown): DocumentParagraph { if (!validDocumentParagraph(value)) throw new Error("Invalid DocumentParagraph response"); return value; }

export type DocumentPartial = {
  "code": DocumentPartialCode;
  "detail": string;
};
function validDocumentPartial(value: unknown): value is DocumentPartial { return (isRecord(value) && (Object.hasOwn(value, "code") && validDocumentPartialCode(value["code"])) && (Object.hasOwn(value, "detail") && (typeof value["detail"] === "string"))); }
export function decodeDocumentPartial(value: unknown): DocumentPartial { if (!validDocumentPartial(value)) throw new Error("Invalid DocumentPartial response"); return value; }

export type DocumentPartialCode = "text_limit" | "block_limit" | "run_limit" | "cell_limit" | "image_limit" | "pixel_budget" | "depth_limit" | "malformed_html" | "unsupported_markup" | "unsupported_dependency" | "missing_image_source" | "external_reference" | "native_analysis_unavailable" | "native_styles_unavailable" | "native_table_structure_unavailable" | "native_partial" | "unverified_provenance" | "other_native_items";
function validDocumentPartialCode(value: unknown): value is DocumentPartialCode { return (typeof value === "string" && (value === "text_limit" || value === "block_limit" || value === "run_limit" || value === "cell_limit" || value === "image_limit" || value === "pixel_budget" || value === "depth_limit" || value === "malformed_html" || value === "unsupported_markup" || value === "unsupported_dependency" || value === "missing_image_source" || value === "external_reference" || value === "native_analysis_unavailable" || value === "native_styles_unavailable" || value === "native_table_structure_unavailable" || value === "native_partial" || value === "unverified_provenance" || value === "other_native_items")); }
export function decodeDocumentPartialCode(value: unknown): DocumentPartialCode { if (!validDocumentPartialCode(value)) throw new Error("Invalid DocumentPartialCode response"); return value; }

export type DocumentRequest = {
  "entry_id": number;
  "generation": number;
  "owner": "history" | "favorites";
  "request_id": string;
};
function validDocumentRequest(value: unknown): value is DocumentRequest { return (isRecord(value) && (Object.hasOwn(value, "entry_id") && (typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"]) && value["entry_id"] >= 1)) && (Object.hasOwn(value, "generation") && (typeof value["generation"] === "number" && Number.isSafeInteger(value["generation"]) && value["generation"] >= 0 && value["generation"] <= 9007199254740991)) && (Object.hasOwn(value, "owner") && (typeof value["owner"] === "string" && (value["owner"] === "history" || value["owner"] === "favorites"))) && (Object.hasOwn(value, "request_id") && (typeof value["request_id"] === "string" && value["request_id"].length >= 1 && value["request_id"].length <= 128))); }
export function decodeDocumentRequest(value: unknown): DocumentRequest { if (!validDocumentRequest(value)) throw new Error("Invalid DocumentRequest response"); return value; }

export type DocumentRun = {
  "link"?: string | null;
  "style": DocumentRunStyle;
  "text": string;
  "unsupported": boolean;
};
function validDocumentRun(value: unknown): value is DocumentRun { return (isRecord(value) && (!Object.hasOwn(value, "link") || ((typeof value["link"] === "string") || (value["link"] === null))) && (Object.hasOwn(value, "style") && validDocumentRunStyle(value["style"])) && (Object.hasOwn(value, "text") && (typeof value["text"] === "string")) && (Object.hasOwn(value, "unsupported") && (typeof value["unsupported"] === "boolean"))); }
export function decodeDocumentRun(value: unknown): DocumentRun { if (!validDocumentRun(value)) throw new Error("Invalid DocumentRun response"); return value; }

export type DocumentRunStyle = {
  "bold": boolean;
  "color"?: DocumentColor | null;
  "highlight"?: DocumentColor | null;
  "italic": boolean;
  "monospace"?: boolean;
  "strikethrough": boolean;
  "subscript": boolean;
  "superscript": boolean;
  "underline": boolean;
};
function validDocumentRunStyle(value: unknown): value is DocumentRunStyle { return (isRecord(value) && (Object.hasOwn(value, "bold") && (typeof value["bold"] === "boolean")) && (!Object.hasOwn(value, "color") || (validDocumentColor(value["color"]) || (value["color"] === null))) && (!Object.hasOwn(value, "highlight") || (validDocumentColor(value["highlight"]) || (value["highlight"] === null))) && (Object.hasOwn(value, "italic") && (typeof value["italic"] === "boolean")) && (!Object.hasOwn(value, "monospace") || (typeof value["monospace"] === "boolean")) && (Object.hasOwn(value, "strikethrough") && (typeof value["strikethrough"] === "boolean")) && (Object.hasOwn(value, "subscript") && (typeof value["subscript"] === "boolean")) && (Object.hasOwn(value, "superscript") && (typeof value["superscript"] === "boolean")) && (Object.hasOwn(value, "underline") && (typeof value["underline"] === "boolean"))); }
export function decodeDocumentRunStyle(value: unknown): DocumentRunStyle { if (!validDocumentRunStyle(value)) throw new Error("Invalid DocumentRunStyle response"); return value; }

export type DocumentTable = {
  "rows": Array<DocumentTableRow>;
  "truncated": boolean;
};
function validDocumentTable(value: unknown): value is DocumentTable { return (isRecord(value) && (Object.hasOwn(value, "rows") && (Array.isArray(value["rows"]) && value["rows"].every(item => validDocumentTableRow(item)))) && (Object.hasOwn(value, "truncated") && (typeof value["truncated"] === "boolean"))); }
export function decodeDocumentTable(value: unknown): DocumentTable { if (!validDocumentTable(value)) throw new Error("Invalid DocumentTable response"); return value; }

export type DocumentTableCell = {
  "header": boolean;
  "runs": Array<DocumentRun>;
};
function validDocumentTableCell(value: unknown): value is DocumentTableCell { return (isRecord(value) && (Object.hasOwn(value, "header") && (typeof value["header"] === "boolean")) && (Object.hasOwn(value, "runs") && (Array.isArray(value["runs"]) && value["runs"].every(item => validDocumentRun(item))))); }
export function decodeDocumentTableCell(value: unknown): DocumentTableCell { if (!validDocumentTableCell(value)) throw new Error("Invalid DocumentTableCell response"); return value; }

export type DocumentTableRow = {
  "cells": Array<DocumentTableCell>;
};
function validDocumentTableRow(value: unknown): value is DocumentTableRow { return (isRecord(value) && (Object.hasOwn(value, "cells") && (Array.isArray(value["cells"]) && value["cells"].every(item => validDocumentTableCell(item))))); }
export function decodeDocumentTableRow(value: unknown): DocumentTableRow { if (!validDocumentTableRow(value)) throw new Error("Invalid DocumentTableRow response"); return value; }

export type FavoriteMutation = {
  "affected_ids": Array<number>;
  "favorite": boolean;
};
function validFavoriteMutation(value: unknown): value is FavoriteMutation { return (isRecord(value) && (Object.hasOwn(value, "affected_ids") && (Array.isArray(value["affected_ids"]) && value["affected_ids"].every(item => (typeof item === "number" && Number.isSafeInteger(item))))) && (Object.hasOwn(value, "favorite") && (typeof value["favorite"] === "boolean"))); }
export function decodeFavoriteMutation(value: unknown): FavoriteMutation { if (!validFavoriteMutation(value)) throw new Error("Invalid FavoriteMutation response"); return value; }

export type FileProgress = {
  "active": boolean;
  "batch_id": string;
  "can_stop": boolean;
  "completed_files": number;
  "device": string;
  "direction": "sending" | "receiving";
  "name": string;
  "sent": number;
  "speed_bytes_per_second": number;
  "status": string;
  "total": number;
  "total_files": number;
};
function validFileProgress(value: unknown): value is FileProgress { return (isRecord(value) && (Object.hasOwn(value, "active") && (typeof value["active"] === "boolean")) && (Object.hasOwn(value, "batch_id") && (typeof value["batch_id"] === "string")) && (Object.hasOwn(value, "can_stop") && (typeof value["can_stop"] === "boolean")) && (Object.hasOwn(value, "completed_files") && (typeof value["completed_files"] === "number" && Number.isSafeInteger(value["completed_files"]) && value["completed_files"] >= 0)) && (Object.hasOwn(value, "device") && (typeof value["device"] === "string")) && (Object.hasOwn(value, "direction") && (typeof value["direction"] === "string" && (value["direction"] === "sending" || value["direction"] === "receiving"))) && (Object.hasOwn(value, "name") && (typeof value["name"] === "string")) && (Object.hasOwn(value, "sent") && (typeof value["sent"] === "number" && Number.isSafeInteger(value["sent"]) && value["sent"] >= 0)) && (Object.hasOwn(value, "speed_bytes_per_second") && (typeof value["speed_bytes_per_second"] === "number" && Number.isSafeInteger(value["speed_bytes_per_second"]) && value["speed_bytes_per_second"] >= 0)) && (Object.hasOwn(value, "status") && (typeof value["status"] === "string")) && (Object.hasOwn(value, "total") && (typeof value["total"] === "number" && Number.isSafeInteger(value["total"]) && value["total"] >= 0)) && (Object.hasOwn(value, "total_files") && (typeof value["total_files"] === "number" && Number.isSafeInteger(value["total_files"]) && value["total_files"] >= 0))); }
export function decodeFileProgress(value: unknown): FileProgress { if (!validFileProgress(value)) throw new Error("Invalid FileProgress response"); return value; }

export type HistoryDisplayInfo = {
  "cache_key": string;
  "kind": "image" | "document" | "text" | "file" | "unknown";
  "thumbnail_eligible": boolean;
  "version": number;
};
function validHistoryDisplayInfo(value: unknown): value is HistoryDisplayInfo { return (isRecord(value) && (Object.hasOwn(value, "cache_key") && (typeof value["cache_key"] === "string")) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string" && (value["kind"] === "image" || value["kind"] === "document" || value["kind"] === "text" || value["kind"] === "file" || value["kind"] === "unknown"))) && (Object.hasOwn(value, "thumbnail_eligible") && (typeof value["thumbnail_eligible"] === "boolean")) && (Object.hasOwn(value, "version") && (typeof value["version"] === "number" && Number.isSafeInteger(value["version"]) && value["version"] === 1))); }
export function decodeHistoryDisplayInfo(value: unknown): HistoryDisplayInfo { if (!validHistoryDisplayInfo(value)) throw new Error("Invalid HistoryDisplayInfo response"); return value; }

export type HistoryEntry = {
  "batch_count": number | null;
  "batch_id": string | null;
  "batch_index": number | null;
  "batch_status": "complete" | "incomplete";
  "batch_total": number | null;
  "categories": Array<string>;
  "category": "text" | "website" | "code" | "command" | "structured_data" | "path" | "image" | "file";
  "category_confidence": number;
  "classifier_version": number;
  "composite"?: CompositeInfo | null;
  "data_hash": string;
  "description": string;
  "display"?: HistoryDisplayInfo | null;
  "id": number;
  "pinned": boolean;
  "size_bytes": number;
  "source_peer": string;
  "timestamp": string;
  "type": "text" | "image" | "file" | "composite";
};
function validHistoryEntry(value: unknown): value is HistoryEntry { return (isRecord(value) && (Object.hasOwn(value, "batch_count") && ((typeof value["batch_count"] === "number" && Number.isSafeInteger(value["batch_count"])) || (value["batch_count"] === null))) && (Object.hasOwn(value, "batch_id") && ((typeof value["batch_id"] === "string") || (value["batch_id"] === null))) && (Object.hasOwn(value, "batch_index") && ((typeof value["batch_index"] === "number" && Number.isSafeInteger(value["batch_index"])) || (value["batch_index"] === null))) && (Object.hasOwn(value, "batch_status") && (typeof value["batch_status"] === "string" && (value["batch_status"] === "complete" || value["batch_status"] === "incomplete"))) && (Object.hasOwn(value, "batch_total") && ((typeof value["batch_total"] === "number" && Number.isSafeInteger(value["batch_total"])) || (value["batch_total"] === null))) && (Object.hasOwn(value, "categories") && (Array.isArray(value["categories"]) && value["categories"].every(item => (typeof item === "string")))) && (Object.hasOwn(value, "category") && (typeof value["category"] === "string" && (value["category"] === "text" || value["category"] === "website" || value["category"] === "code" || value["category"] === "command" || value["category"] === "structured_data" || value["category"] === "path" || value["category"] === "image" || value["category"] === "file"))) && (Object.hasOwn(value, "category_confidence") && (typeof value["category_confidence"] === "number" && Number.isSafeInteger(value["category_confidence"]))) && (Object.hasOwn(value, "classifier_version") && (typeof value["classifier_version"] === "number" && Number.isSafeInteger(value["classifier_version"]))) && (!Object.hasOwn(value, "composite") || (validCompositeInfo(value["composite"]) || (value["composite"] === null))) && (Object.hasOwn(value, "data_hash") && (typeof value["data_hash"] === "string")) && (Object.hasOwn(value, "description") && (typeof value["description"] === "string")) && (!Object.hasOwn(value, "display") || (validHistoryDisplayInfo(value["display"]) || (value["display"] === null))) && (Object.hasOwn(value, "id") && (typeof value["id"] === "number" && Number.isSafeInteger(value["id"]))) && (Object.hasOwn(value, "pinned") && (typeof value["pinned"] === "boolean")) && (Object.hasOwn(value, "size_bytes") && (typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]))) && (Object.hasOwn(value, "source_peer") && (typeof value["source_peer"] === "string")) && (Object.hasOwn(value, "timestamp") && (typeof value["timestamp"] === "string")) && (Object.hasOwn(value, "type") && (typeof value["type"] === "string" && (value["type"] === "text" || value["type"] === "image" || value["type"] === "file" || value["type"] === "composite")))); }
export function decodeHistoryEntry(value: unknown): HistoryEntry { if (!validHistoryEntry(value)) throw new Error("Invalid HistoryEntry response"); return value; }

export type HistoryIdentity = {
  "archive_digest": string;
  "archive_revision": number;
  "capture_id": string;
  "content_id": string;
  "dedupe_key": string | null;
  "version": number;
};
function validHistoryIdentity(value: unknown): value is HistoryIdentity { return (isRecord(value) && (Object.hasOwn(value, "archive_digest") && (typeof value["archive_digest"] === "string")) && (Object.hasOwn(value, "archive_revision") && (typeof value["archive_revision"] === "number" && Number.isSafeInteger(value["archive_revision"]) && value["archive_revision"] >= 1 && value["archive_revision"] <= 9007199254740991)) && (Object.hasOwn(value, "capture_id") && (typeof value["capture_id"] === "string")) && (Object.hasOwn(value, "content_id") && (typeof value["content_id"] === "string")) && (Object.hasOwn(value, "dedupe_key") && ((typeof value["dedupe_key"] === "string") || (value["dedupe_key"] === null))) && (Object.hasOwn(value, "version") && (typeof value["version"] === "number" && Number.isSafeInteger(value["version"]) && value["version"] === 1))); }
export function decodeHistoryIdentity(value: unknown): HistoryIdentity { if (!validHistoryIdentity(value)) throw new Error("Invalid HistoryIdentity response"); return value; }

export type HistoryQueryPage = {
  "display_index_pending"?: boolean | null;
  "entries": Array<HistoryEntry>;
  "has_more": boolean;
  "total": number | null;
};
function validHistoryQueryPage(value: unknown): value is HistoryQueryPage { return (isRecord(value) && (!Object.hasOwn(value, "display_index_pending") || ((typeof value["display_index_pending"] === "boolean") || (value["display_index_pending"] === null))) && (Object.hasOwn(value, "entries") && (Array.isArray(value["entries"]) && value["entries"].every(item => validHistoryEntry(item)))) && (Object.hasOwn(value, "has_more") && (typeof value["has_more"] === "boolean")) && (Object.hasOwn(value, "total") && ((typeof value["total"] === "number" && Number.isSafeInteger(value["total"]) && value["total"] >= 0) || (value["total"] === null)))); }
export function decodeHistoryQueryPage(value: unknown): HistoryQueryPage { if (!validHistoryQueryPage(value)) throw new Error("Invalid HistoryQueryPage response"); return value; }

export type HistoryRestoreReport = {
  "complete": boolean;
  "missing_images": number;
};
function validHistoryRestoreReport(value: unknown): value is HistoryRestoreReport { return (isRecord(value) && (Object.hasOwn(value, "complete") && (typeof value["complete"] === "boolean")) && (Object.hasOwn(value, "missing_images") && (typeof value["missing_images"] === "number" && Number.isSafeInteger(value["missing_images"]) && value["missing_images"] >= 0 && value["missing_images"] <= 4096))); }
export function decodeHistoryRestoreReport(value: unknown): HistoryRestoreReport { if (!validHistoryRestoreReport(value)) throw new Error("Invalid HistoryRestoreReport response"); return value; }

export type LocalCapabilities = {
  "document_preview_version"?: number | null;
  "history_display_version"?: number | null;
  "max_preview_bytes": number;
  "platform": string;
  "schema_version": number;
  "supports_binary_preview": boolean;
  "supports_runtime_snapshot": boolean;
  "supports_stable_errors": boolean;
  "wire_version": number;
};
function validLocalCapabilities(value: unknown): value is LocalCapabilities { return (isRecord(value) && (!Object.hasOwn(value, "document_preview_version") || ((typeof value["document_preview_version"] === "number" && Number.isSafeInteger(value["document_preview_version"]) && value["document_preview_version"] === 1) || (value["document_preview_version"] === null))) && (!Object.hasOwn(value, "history_display_version") || ((typeof value["history_display_version"] === "number" && Number.isSafeInteger(value["history_display_version"]) && value["history_display_version"] === 1) || (value["history_display_version"] === null))) && (Object.hasOwn(value, "max_preview_bytes") && (typeof value["max_preview_bytes"] === "number" && Number.isSafeInteger(value["max_preview_bytes"]) && value["max_preview_bytes"] === 67108864)) && (Object.hasOwn(value, "platform") && (typeof value["platform"] === "string")) && (Object.hasOwn(value, "schema_version") && (typeof value["schema_version"] === "number" && Number.isSafeInteger(value["schema_version"]) && value["schema_version"] === 2)) && (Object.hasOwn(value, "supports_binary_preview") && (typeof value["supports_binary_preview"] === "boolean")) && (Object.hasOwn(value, "supports_runtime_snapshot") && (typeof value["supports_runtime_snapshot"] === "boolean")) && (Object.hasOwn(value, "supports_stable_errors") && (typeof value["supports_stable_errors"] === "boolean")) && (Object.hasOwn(value, "wire_version") && (typeof value["wire_version"] === "number" && Number.isSafeInteger(value["wire_version"]) && value["wire_version"] >= 4 && value["wire_version"] <= 5))); }
export function decodeLocalCapabilities(value: unknown): LocalCapabilities { if (!validLocalCapabilities(value)) throw new Error("Invalid LocalCapabilities response"); return value; }

export type LocalDeviceSnapshot = {
  "connection_mode": string;
  "fingerprint": string;
  "hostname": string;
  "iroh_endpoint_id": string | null;
  "public_key": string;
  "routes": Array<PeerRouteSnapshot>;
  "tailscale_ip": string;
};
function validLocalDeviceSnapshot(value: unknown): value is LocalDeviceSnapshot { return (isRecord(value) && (Object.hasOwn(value, "connection_mode") && (typeof value["connection_mode"] === "string")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "iroh_endpoint_id") && ((typeof value["iroh_endpoint_id"] === "string") || (value["iroh_endpoint_id"] === null))) && (Object.hasOwn(value, "public_key") && (typeof value["public_key"] === "string")) && (Object.hasOwn(value, "routes") && (Array.isArray(value["routes"]) && value["routes"].every(item => validPeerRouteSnapshot(item)))) && (Object.hasOwn(value, "tailscale_ip") && (typeof value["tailscale_ip"] === "string"))); }
export function decodeLocalDeviceSnapshot(value: unknown): LocalDeviceSnapshot { if (!validLocalDeviceSnapshot(value)) throw new Error("Invalid LocalDeviceSnapshot response"); return value; }

export type MacRuntimeSnapshot = {
  "history_version": number;
  "notification_dropped_total": number;
  "notification_earliest_available_id": number;
  "notifications": Array<RuntimeNotification>;
  "progress": FileProgress | null;
  "revision": number;
  "service_instance": number;
  "status": DaemonStatus;
  "storage": StorageStatus;
  "sync_enabled": boolean;
};
function validMacRuntimeSnapshot(value: unknown): value is MacRuntimeSnapshot { return (isRecord(value) && (Object.hasOwn(value, "history_version") && (typeof value["history_version"] === "number" && Number.isSafeInteger(value["history_version"]) && value["history_version"] >= 0)) && (Object.hasOwn(value, "notification_dropped_total") && (typeof value["notification_dropped_total"] === "number" && Number.isSafeInteger(value["notification_dropped_total"]) && value["notification_dropped_total"] >= 0)) && (Object.hasOwn(value, "notification_earliest_available_id") && (typeof value["notification_earliest_available_id"] === "number" && Number.isSafeInteger(value["notification_earliest_available_id"]) && value["notification_earliest_available_id"] >= 0)) && (Object.hasOwn(value, "notifications") && (Array.isArray(value["notifications"]) && value["notifications"].every(item => validRuntimeNotification(item)))) && (Object.hasOwn(value, "progress") && (validFileProgress(value["progress"]) || (value["progress"] === null))) && (Object.hasOwn(value, "revision") && (typeof value["revision"] === "number" && Number.isSafeInteger(value["revision"]) && value["revision"] >= 0)) && (Object.hasOwn(value, "service_instance") && (typeof value["service_instance"] === "number" && Number.isSafeInteger(value["service_instance"]) && value["service_instance"] >= 0)) && (Object.hasOwn(value, "status") && validDaemonStatus(value["status"])) && (Object.hasOwn(value, "storage") && validStorageStatus(value["storage"])) && (Object.hasOwn(value, "sync_enabled") && (typeof value["sync_enabled"] === "boolean"))); }
export function decodeMacRuntimeSnapshot(value: unknown): MacRuntimeSnapshot { if (!validMacRuntimeSnapshot(value)) throw new Error("Invalid MacRuntimeSnapshot response"); return value; }

export type PairingFailureKind = "protocol_incompatible" | "identity_mismatch" | "window_closed" | "timed_out" | "connection_closed" | "invalid_advertisement" | "invalid_argument" | "busy" | "cancelled" | "transport" | "unknown";
function validPairingFailureKind(value: unknown): value is PairingFailureKind { return typeof value === "string"; }
export function decodePairingFailureKind(value: unknown): PairingFailureKind { if (!validPairingFailureKind(value)) throw new Error("Invalid PairingFailureKind response"); return ["protocol_incompatible","identity_mismatch","window_closed","timed_out","connection_closed","invalid_advertisement","invalid_argument","busy","cancelled","transport","unknown"].includes(value as PairingFailureKind) ? value as PairingFailureKind : "unknown"; }

export type PairingPeerStatus = {
  "address": string;
  "fingerprint": string;
  "hostname": string;
  "local_confirmed": boolean;
  "remote_confirmed": boolean;
  "verification_code": string;
};
function validPairingPeerStatus(value: unknown): value is PairingPeerStatus { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "local_confirmed") && (typeof value["local_confirmed"] === "boolean")) && (Object.hasOwn(value, "remote_confirmed") && (typeof value["remote_confirmed"] === "boolean")) && (Object.hasOwn(value, "verification_code") && (typeof value["verification_code"] === "string"))); }
export function decodePairingPeerStatus(value: unknown): PairingPeerStatus { if (!validPairingPeerStatus(value)) throw new Error("Invalid PairingPeerStatus response"); return value; }

export type PairingPhase = "disabled" | "waiting" | "handshaking" | "verification" | "waiting_for_peer" | "finalizing" | "paired" | "cancelled" | "timed_out" | "locked";
function validPairingPhase(value: unknown): value is PairingPhase { return (typeof value === "string" && (value === "disabled" || value === "waiting" || value === "handshaking" || value === "verification" || value === "waiting_for_peer" || value === "finalizing" || value === "paired" || value === "cancelled" || value === "timed_out" || value === "locked")); }
export function decodePairingPhase(value: unknown): PairingPhase { if (!validPairingPhase(value)) throw new Error("Invalid PairingPhase response"); return value; }

export type PairingStatus = {
  "error": string | null;
  "expires_at": number | null;
  "failed_attempts": number;
  "failure_kind"?: PairingFailureKind | null;
  "max_failures": number;
  "pairing_enabled": boolean;
  "peer": PairingPeerStatus | null;
  "pending": Array<PendingPairingSummary>;
  "pending_store_unavailable": boolean;
  "phase": PairingPhase;
  "remaining_seconds": number;
};
function validPairingStatus(value: unknown): value is PairingStatus { return (isRecord(value) && (Object.hasOwn(value, "error") && ((typeof value["error"] === "string") || (value["error"] === null))) && (Object.hasOwn(value, "expires_at") && ((typeof value["expires_at"] === "number" && Number.isSafeInteger(value["expires_at"]) && value["expires_at"] >= 0) || (value["expires_at"] === null))) && (Object.hasOwn(value, "failed_attempts") && (typeof value["failed_attempts"] === "number" && Number.isSafeInteger(value["failed_attempts"]) && value["failed_attempts"] >= 0 && value["failed_attempts"] <= 255)) && (!Object.hasOwn(value, "failure_kind") || ((typeof value["failure_kind"] === "string") || (value["failure_kind"] === null))) && (Object.hasOwn(value, "max_failures") && (typeof value["max_failures"] === "number" && Number.isSafeInteger(value["max_failures"]) && value["max_failures"] >= 0 && value["max_failures"] <= 255)) && (Object.hasOwn(value, "pairing_enabled") && (typeof value["pairing_enabled"] === "boolean")) && (Object.hasOwn(value, "peer") && (validPairingPeerStatus(value["peer"]) || (value["peer"] === null))) && (Object.hasOwn(value, "pending") && (Array.isArray(value["pending"]) && value["pending"].every(item => validPendingPairingSummary(item)))) && (Object.hasOwn(value, "pending_store_unavailable") && (typeof value["pending_store_unavailable"] === "boolean")) && (Object.hasOwn(value, "phase") && validPairingPhase(value["phase"])) && (Object.hasOwn(value, "remaining_seconds") && (typeof value["remaining_seconds"] === "number" && Number.isSafeInteger(value["remaining_seconds"]) && value["remaining_seconds"] >= 0))); }
function normalizeFailureKind(value: unknown): PairingFailureKind | null { if (value === null) return null; return ["protocol_incompatible","identity_mismatch","window_closed","timed_out","connection_closed","invalid_advertisement","invalid_argument","busy","cancelled","transport","unknown"].includes(value as PairingFailureKind) ? value as PairingFailureKind : "unknown"; }
export function decodePairingStatus(value: unknown): PairingStatus { if (!validPairingStatus(value)) throw new Error("Invalid PairingStatus response"); return !Object.hasOwn(value, "failure_kind") || value["failure_kind"] === null ? value : { ...value, failure_kind: normalizeFailureKind(value["failure_kind"]) }; }

export type PeerCandidate = {
  "address": string;
  "interface": ConnectionInterface;
  "latency"?: number | null;
  "online": boolean;
  "priority": number;
  "rtt_capable": boolean;
  "status": PeerStatus;
};
function validPeerCandidate(value: unknown): value is PeerCandidate { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (!Object.hasOwn(value, "latency") || ((typeof value["latency"] === "number" && Number.isSafeInteger(value["latency"]) && value["latency"] >= 0) || (value["latency"] === null))) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "priority") && (typeof value["priority"] === "number" && Number.isSafeInteger(value["priority"]) && value["priority"] >= 0 && value["priority"] <= 255)) && (Object.hasOwn(value, "rtt_capable") && (typeof value["rtt_capable"] === "boolean")) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"]))); }
export function decodePeerCandidate(value: unknown): PeerCandidate { if (!validPeerCandidate(value)) throw new Error("Invalid PeerCandidate response"); return value; }

export type PeerRouteSnapshot = {
  "address": string;
  "connected": boolean;
  "interface": ConnectionInterface;
  "latency_ms": number | null;
  "online": boolean;
  "pairing_endpoint": boolean;
  "rtt_capable": boolean;
  "status": PeerStatus;
};
function validPeerRouteSnapshot(value: unknown): value is PeerRouteSnapshot { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "connected") && (typeof value["connected"] === "boolean")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (Object.hasOwn(value, "latency_ms") && ((typeof value["latency_ms"] === "number" && Number.isSafeInteger(value["latency_ms"]) && value["latency_ms"] >= 0) || (value["latency_ms"] === null))) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "pairing_endpoint") && (typeof value["pairing_endpoint"] === "boolean")) && (Object.hasOwn(value, "rtt_capable") && (typeof value["rtt_capable"] === "boolean")) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"]))); }
export function decodePeerRouteSnapshot(value: unknown): PeerRouteSnapshot { if (!validPeerRouteSnapshot(value)) throw new Error("Invalid PeerRouteSnapshot response"); return value; }

export type PeerSnapshot = {
  "address": string;
  "candidates": Array<PeerCandidate>;
  "connection_mode": string;
  "current_address"?: string | null;
  "current_interface"?: ConnectionInterface | null;
  "enabled": boolean;
  "fingerprint": string;
  "hostname": string;
  "online": boolean;
  "protocol_error": string | null;
  "required_protocol_version": number | null;
  "routes": Array<PeerRouteSnapshot>;
  "status": PeerStatus;
  "tailscale_ip": string;
  "trusted": boolean;
};
function validPeerSnapshot(value: unknown): value is PeerSnapshot { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "candidates") && (Array.isArray(value["candidates"]) && value["candidates"].every(item => validPeerCandidate(item)))) && (Object.hasOwn(value, "connection_mode") && (typeof value["connection_mode"] === "string")) && (!Object.hasOwn(value, "current_address") || ((typeof value["current_address"] === "string") || (value["current_address"] === null))) && (!Object.hasOwn(value, "current_interface") || (validConnectionInterface(value["current_interface"]) || (value["current_interface"] === null))) && (Object.hasOwn(value, "enabled") && (typeof value["enabled"] === "boolean")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "protocol_error") && ((typeof value["protocol_error"] === "string") || (value["protocol_error"] === null))) && (Object.hasOwn(value, "required_protocol_version") && ((typeof value["required_protocol_version"] === "number" && Number.isSafeInteger(value["required_protocol_version"]) && value["required_protocol_version"] >= 0 && value["required_protocol_version"] <= 255) || (value["required_protocol_version"] === null))) && (Object.hasOwn(value, "routes") && (Array.isArray(value["routes"]) && value["routes"].every(item => validPeerRouteSnapshot(item)))) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"])) && (Object.hasOwn(value, "tailscale_ip") && (typeof value["tailscale_ip"] === "string")) && (Object.hasOwn(value, "trusted") && (typeof value["trusted"] === "boolean"))); }
export function decodePeerSnapshot(value: unknown): PeerSnapshot { if (!validPeerSnapshot(value)) throw new Error("Invalid PeerSnapshot response"); return value; }

export type PeerStatus = "discovered" | "confirming" | "online" | "connected" | "offline";
function validPeerStatus(value: unknown): value is PeerStatus { return (typeof value === "string" && (value === "discovered" || value === "confirming" || value === "online" || value === "connected" || value === "offline")); }
export function decodePeerStatus(value: unknown): PeerStatus { if (!validPeerStatus(value)) throw new Error("Invalid PeerStatus response"); return value; }

export type PeersResponse = {
  "discovery_error": string | null;
  "paired_peer_endpoints": Record<string, string>;
  "peers": Array<PeerSnapshot>;
  "self": LocalDeviceSnapshot;
};
function validPeersResponse(value: unknown): value is PeersResponse { return (isRecord(value) && (Object.hasOwn(value, "discovery_error") && ((typeof value["discovery_error"] === "string") || (value["discovery_error"] === null))) && (Object.hasOwn(value, "paired_peer_endpoints") && (isRecord(value["paired_peer_endpoints"]) && Object.values(value["paired_peer_endpoints"]).every(item => (typeof item === "string")))) && (Object.hasOwn(value, "peers") && (Array.isArray(value["peers"]) && value["peers"].every(item => validPeerSnapshot(item)))) && (Object.hasOwn(value, "self") && validLocalDeviceSnapshot(value["self"]))); }
export function decodePeersResponse(value: unknown): PeersResponse { if (!validPeersResponse(value)) throw new Error("Invalid PeersResponse response"); return value; }

export type PendingPairingSummary = {
  "address": string;
  "fingerprint": string;
  "hostname": string;
  "interface": string;
  "locally_trusted": boolean;
};
function validPendingPairingSummary(value: unknown): value is PendingPairingSummary { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "interface") && (typeof value["interface"] === "string")) && (Object.hasOwn(value, "locally_trusted") && (typeof value["locally_trusted"] === "boolean"))); }
export function decodePendingPairingSummary(value: unknown): PendingPairingSummary { if (!validPendingPairingSummary(value)) throw new Error("Invalid PendingPairingSummary response"); return value; }

export type PreviewBatchNavigation = {
  "batch_id": string;
  "first_entry_id": number;
  "item_count": number;
  "item_index": number;
  "last_entry_id": number;
  "next_entry_id": number | null;
  "previous_entry_id": number | null;
};
function validPreviewBatchNavigation(value: unknown): value is PreviewBatchNavigation { return (isRecord(value) && (Object.hasOwn(value, "batch_id") && (typeof value["batch_id"] === "string")) && (Object.hasOwn(value, "first_entry_id") && (typeof value["first_entry_id"] === "number" && Number.isSafeInteger(value["first_entry_id"]))) && (Object.hasOwn(value, "item_count") && (typeof value["item_count"] === "number" && Number.isSafeInteger(value["item_count"]) && value["item_count"] >= 0)) && (Object.hasOwn(value, "item_index") && (typeof value["item_index"] === "number" && Number.isSafeInteger(value["item_index"]) && value["item_index"] >= 0)) && (Object.hasOwn(value, "last_entry_id") && (typeof value["last_entry_id"] === "number" && Number.isSafeInteger(value["last_entry_id"]))) && (Object.hasOwn(value, "next_entry_id") && ((typeof value["next_entry_id"] === "number" && Number.isSafeInteger(value["next_entry_id"])) || (value["next_entry_id"] === null))) && (Object.hasOwn(value, "previous_entry_id") && ((typeof value["previous_entry_id"] === "number" && Number.isSafeInteger(value["previous_entry_id"])) || (value["previous_entry_id"] === null)))); }
export function decodePreviewBatchNavigation(value: unknown): PreviewBatchNavigation { if (!validPreviewBatchNavigation(value)) throw new Error("Invalid PreviewBatchNavigation response"); return value; }

export type PreviewErrorCode = "entry_not_found" | "batch_not_found" | "entry_not_in_batch" | "metadata_unavailable" | "payload_unavailable" | "preview_too_large" | "unsupported_type" | "invalid_size";
function validPreviewErrorCode(value: unknown): value is PreviewErrorCode { return (typeof value === "string" && (value === "entry_not_found" || value === "batch_not_found" || value === "entry_not_in_batch" || value === "metadata_unavailable" || value === "payload_unavailable" || value === "preview_too_large" || value === "unsupported_type" || value === "invalid_size")); }
export function decodePreviewErrorCode(value: unknown): PreviewErrorCode { if (!validPreviewErrorCode(value)) throw new Error("Invalid PreviewErrorCode response"); return value; }

export type PreviewErrorInfo = {
  "code": PreviewErrorCode;
  "entry_id": number | null;
  "limit_bytes": number | null;
  "message": string;
  "retryable": boolean;
  "size_bytes": number | null;
};
function validPreviewErrorInfo(value: unknown): value is PreviewErrorInfo { return (isRecord(value) && (Object.hasOwn(value, "code") && validPreviewErrorCode(value["code"])) && (Object.hasOwn(value, "entry_id") && ((typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"])) || (value["entry_id"] === null))) && (Object.hasOwn(value, "limit_bytes") && ((typeof value["limit_bytes"] === "number" && Number.isSafeInteger(value["limit_bytes"]) && value["limit_bytes"] >= 0) || (value["limit_bytes"] === null))) && (Object.hasOwn(value, "message") && (typeof value["message"] === "string")) && (Object.hasOwn(value, "retryable") && (typeof value["retryable"] === "boolean")) && (Object.hasOwn(value, "size_bytes") && ((typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]) && value["size_bytes"] >= 0) || (value["size_bytes"] === null)))); }
export function decodePreviewErrorInfo(value: unknown): PreviewErrorInfo { if (!validPreviewErrorInfo(value)) throw new Error("Invalid PreviewErrorInfo response"); return value; }

export type PreviewFrameMetadata = {
  "batch": PreviewBatchNavigation | null;
  "entry_id": number;
  "height": number | null;
  "kind": "text" | "image" | "file";
  "name": string;
  "request_id"?: string | null;
  "size_bytes": number;
  "width": number | null;
};
function validPreviewFrameMetadata(value: unknown): value is PreviewFrameMetadata { return (isRecord(value) && (Object.hasOwn(value, "batch") && (validPreviewBatchNavigation(value["batch"]) || (value["batch"] === null))) && (Object.hasOwn(value, "entry_id") && (typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"]) && value["entry_id"] >= 1)) && (Object.hasOwn(value, "height") && ((typeof value["height"] === "number" && Number.isSafeInteger(value["height"]) && value["height"] >= 0) || (value["height"] === null))) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string" && (value["kind"] === "text" || value["kind"] === "image" || value["kind"] === "file"))) && (Object.hasOwn(value, "name") && (typeof value["name"] === "string" && value["name"].length >= 1)) && (!Object.hasOwn(value, "request_id") || ((typeof value["request_id"] === "string") || (value["request_id"] === null))) && (Object.hasOwn(value, "size_bytes") && (typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]) && value["size_bytes"] >= 0 && value["size_bytes"] <= 67108864)) && (Object.hasOwn(value, "width") && ((typeof value["width"] === "number" && Number.isSafeInteger(value["width"]) && value["width"] >= 0) || (value["width"] === null)))); }
export function decodePreviewFrameMetadata(value: unknown): PreviewFrameMetadata { if (!validPreviewFrameMetadata(value)) throw new Error("Invalid PreviewFrameMetadata response"); return value; }

export type RuntimeNotification = {
  "arrival"?: ClipboardArrival | null;
  "code"?: string | null;
  "id": number;
  "level": string;
  "message": string;
};
function validRuntimeNotification(value: unknown): value is RuntimeNotification { return (isRecord(value) && (!Object.hasOwn(value, "arrival") || (validClipboardArrival(value["arrival"]) || (value["arrival"] === null))) && (!Object.hasOwn(value, "code") || ((typeof value["code"] === "string") || (value["code"] === null))) && (Object.hasOwn(value, "id") && (typeof value["id"] === "number" && Number.isSafeInteger(value["id"]) && value["id"] >= 0)) && (Object.hasOwn(value, "level") && (typeof value["level"] === "string")) && (Object.hasOwn(value, "message") && (typeof value["message"] === "string"))); }
export function decodeRuntimeNotification(value: unknown): RuntimeNotification { if (!validRuntimeNotification(value)) throw new Error("Invalid RuntimeNotification response"); return value; }

export type SourceContextGrantReport = {
  "budget_bytes": number;
  "expires_at": number;
  "grant_id": string;
  "node_count": number;
  "origin": string;
  "ticket": string;
};
function validSourceContextGrantReport(value: unknown): value is SourceContextGrantReport { return (isRecord(value) && (Object.hasOwn(value, "budget_bytes") && (typeof value["budget_bytes"] === "number" && Number.isSafeInteger(value["budget_bytes"]) && value["budget_bytes"] >= 0)) && (Object.hasOwn(value, "expires_at") && (typeof value["expires_at"] === "number" && Number.isSafeInteger(value["expires_at"]) && value["expires_at"] >= 0)) && (Object.hasOwn(value, "grant_id") && (typeof value["grant_id"] === "string")) && (Object.hasOwn(value, "node_count") && (typeof value["node_count"] === "number" && Number.isSafeInteger(value["node_count"]) && value["node_count"] >= 0 && value["node_count"] <= 65535)) && (Object.hasOwn(value, "origin") && (typeof value["origin"] === "string")) && (Object.hasOwn(value, "ticket") && (typeof value["ticket"] === "string"))); }
export function decodeSourceContextGrantReport(value: unknown): SourceContextGrantReport { if (!validSourceContextGrantReport(value)) throw new Error("Invalid SourceContextGrantReport response"); return value; }

export type SourceContextImportReport = {
  "complete": boolean;
  "deduplicated": number;
  "failures": Array<ArchiveResourceFailure>;
  "imported": number;
};
function validSourceContextImportReport(value: unknown): value is SourceContextImportReport { return (isRecord(value) && (Object.hasOwn(value, "complete") && (typeof value["complete"] === "boolean")) && (Object.hasOwn(value, "deduplicated") && (typeof value["deduplicated"] === "number" && Number.isSafeInteger(value["deduplicated"]) && value["deduplicated"] >= 0 && value["deduplicated"] <= 65535)) && (Object.hasOwn(value, "failures") && (Array.isArray(value["failures"]) && value["failures"].every(item => validArchiveResourceFailure(item)))) && (Object.hasOwn(value, "imported") && (typeof value["imported"] === "number" && Number.isSafeInteger(value["imported"]) && value["imported"] >= 0 && value["imported"] <= 65535))); }
export function decodeSourceContextImportReport(value: unknown): SourceContextImportReport { if (!validSourceContextImportReport(value)) throw new Error("Invalid SourceContextImportReport response"); return value; }

export type SourceContextRevokeReport = {
  "revoked": boolean;
};
function validSourceContextRevokeReport(value: unknown): value is SourceContextRevokeReport { return (isRecord(value) && (Object.hasOwn(value, "revoked") && (typeof value["revoked"] === "boolean"))); }
export function decodeSourceContextRevokeReport(value: unknown): SourceContextRevokeReport { if (!validSourceContextRevokeReport(value)) throw new Error("Invalid SourceContextRevokeReport response"); return value; }

export type StableErrorCode = "invalid_argument" | "not_found" | "temporarily_busy" | "storage_unavailable" | "unauthorized" | "protocol_incompatible" | "publish_partial" | "publish_unknown" | "publish_superseded" | "native_only" | "originals_incomplete" | "resources_incomplete" | "no_single_image" | "format_unavailable" | "unsupported_format" | "pairing_identity_mismatch" | "pairing_window_closed" | "pairing_timed_out" | "pairing_connection_closed" | "pairing_busy" | "pairing_cancelled" | "internal_error";
function validStableErrorCode(value: unknown): value is StableErrorCode { return typeof value === "string"; }
export function decodeStableErrorCode(value: unknown): StableErrorCode { if (!validStableErrorCode(value)) throw new Error("Invalid StableErrorCode response"); return ["invalid_argument","not_found","temporarily_busy","storage_unavailable","unauthorized","protocol_incompatible","publish_partial","publish_unknown","publish_superseded","native_only","originals_incomplete","resources_incomplete","no_single_image","format_unavailable","unsupported_format","pairing_identity_mismatch","pairing_window_closed","pairing_timed_out","pairing_connection_closed","pairing_busy","pairing_cancelled","internal_error"].includes(value as StableErrorCode) ? value as StableErrorCode : "internal_error"; }

export type StableErrorDetailClass = "request" | "resource" | "contention" | "storage" | "authorization" | "protocol" | "internal";
function validStableErrorDetailClass(value: unknown): value is StableErrorDetailClass { return (typeof value === "string" && (value === "request" || value === "resource" || value === "contention" || value === "storage" || value === "authorization" || value === "protocol" || value === "internal")); }
export function decodeStableErrorDetailClass(value: unknown): StableErrorDetailClass { if (!validStableErrorDetailClass(value)) throw new Error("Invalid StableErrorDetailClass response"); return value; }

export type StableErrorEnvelope = {
  "code": StableErrorCode;
  "detail_class": StableErrorDetailClass;
  "message_key": string;
  "retryable": boolean;
  "schema_version": number;
};
function validStableErrorEnvelope(value: unknown): value is StableErrorEnvelope { return (isRecord(value) && (Object.hasOwn(value, "code") && validStableErrorCode(value["code"])) && (Object.hasOwn(value, "detail_class") && (typeof value["detail_class"] === "string")) && (Object.hasOwn(value, "message_key") && (typeof value["message_key"] === "string")) && (Object.hasOwn(value, "retryable") && (typeof value["retryable"] === "boolean")) && (Object.hasOwn(value, "schema_version") && (typeof value["schema_version"] === "number" && Number.isSafeInteger(value["schema_version"]) && value["schema_version"] === 1))); }
export function decodeStableErrorEnvelope(value: unknown): StableErrorEnvelope { if (!validStableErrorEnvelope(value)) throw new Error("Invalid StableErrorEnvelope response"); const code = decodeStableErrorCode(value.code); const policy = ({"invalid_argument":{"retryable":false,"message_key":"error.invalid_argument","detail_class":"request"},"not_found":{"retryable":false,"message_key":"error.not_found","detail_class":"resource"},"temporarily_busy":{"retryable":true,"message_key":"error.temporarily_busy","detail_class":"contention"},"storage_unavailable":{"retryable":true,"message_key":"error.storage_unavailable","detail_class":"storage"},"unauthorized":{"retryable":false,"message_key":"error.unauthorized","detail_class":"authorization"},"protocol_incompatible":{"retryable":false,"message_key":"error.protocol_incompatible","detail_class":"protocol"},"publish_partial":{"retryable":true,"message_key":"history.restore.code.publish_partial","detail_class":"resource"},"publish_unknown":{"retryable":true,"message_key":"history.restore.code.publish_unknown","detail_class":"resource"},"publish_superseded":{"retryable":false,"message_key":"history.restore.code.publish_superseded","detail_class":"resource"},"native_only":{"retryable":false,"message_key":"history.restore.code.native_only","detail_class":"resource"},"originals_incomplete":{"retryable":false,"message_key":"history.restore.code.originals_incomplete","detail_class":"resource"},"resources_incomplete":{"retryable":false,"message_key":"history.restore.code.resources_incomplete","detail_class":"resource"},"no_single_image":{"retryable":false,"message_key":"history.restore.code.no_single_image","detail_class":"resource"},"format_unavailable":{"retryable":false,"message_key":"history.restore.code.format_unavailable","detail_class":"resource"},"unsupported_format":{"retryable":false,"message_key":"history.restore.code.unsupported_format","detail_class":"resource"},"pairing_identity_mismatch":{"retryable":false,"message_key":"error.pairing_identity_mismatch","detail_class":"authorization"},"pairing_window_closed":{"retryable":false,"message_key":"error.pairing_window_closed","detail_class":"resource"},"pairing_timed_out":{"retryable":false,"message_key":"error.pairing_timed_out","detail_class":"contention"},"pairing_connection_closed":{"retryable":false,"message_key":"error.pairing_connection_closed","detail_class":"protocol"},"pairing_busy":{"retryable":false,"message_key":"error.pairing_busy","detail_class":"contention"},"pairing_cancelled":{"retryable":false,"message_key":"error.pairing_cancelled","detail_class":"request"},"internal_error":{"retryable":false,"message_key":"error.internal","detail_class":"internal"}} as const)[code]; return { schema_version: value.schema_version, code, ...policy }; }

export type StorageStatus = {
  "available": boolean;
  "error": string | null;
  "quota_bytes": number;
  "root": string;
  "used_bytes": number;
};
function validStorageStatus(value: unknown): value is StorageStatus { return (isRecord(value) && (Object.hasOwn(value, "available") && (typeof value["available"] === "boolean")) && (Object.hasOwn(value, "error") && ((typeof value["error"] === "string") || (value["error"] === null))) && (Object.hasOwn(value, "quota_bytes") && (typeof value["quota_bytes"] === "number" && Number.isSafeInteger(value["quota_bytes"]) && value["quota_bytes"] >= 0)) && (Object.hasOwn(value, "root") && (typeof value["root"] === "string")) && (Object.hasOwn(value, "used_bytes") && (typeof value["used_bytes"] === "number" && Number.isSafeInteger(value["used_bytes"]) && value["used_bytes"] >= 0))); }
export function decodeStorageStatus(value: unknown): StorageStatus { if (!validStorageStatus(value)) throw new Error("Invalid StorageStatus response"); return value; }

export type SyncWarning = {
  "id": number;
  "kind": string;
  "occurred_at_ms": number;
  "peer": string;
};
function validSyncWarning(value: unknown): value is SyncWarning { return (isRecord(value) && (Object.hasOwn(value, "id") && (typeof value["id"] === "number" && Number.isSafeInteger(value["id"]) && value["id"] >= 0)) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string")) && (Object.hasOwn(value, "occurred_at_ms") && (typeof value["occurred_at_ms"] === "number" && Number.isSafeInteger(value["occurred_at_ms"]))) && (Object.hasOwn(value, "peer") && (typeof value["peer"] === "string"))); }
export function decodeSyncWarning(value: unknown): SyncWarning { if (!validSyncWarning(value)) throw new Error("Invalid SyncWarning response"); return value; }

export type WindowsRuntimeSnapshot = {
  "history_version": number;
  "notifications": Array<RuntimeNotification>;
  "progress": FileProgress | null;
  "revision": number;
  "sync_warning": SyncWarning | null;
};
function validWindowsRuntimeSnapshot(value: unknown): value is WindowsRuntimeSnapshot { return (isRecord(value) && (Object.hasOwn(value, "history_version") && (typeof value["history_version"] === "number" && Number.isSafeInteger(value["history_version"]) && value["history_version"] >= 0)) && (Object.hasOwn(value, "notifications") && (Array.isArray(value["notifications"]) && value["notifications"].every(item => validRuntimeNotification(item)))) && (Object.hasOwn(value, "progress") && (validFileProgress(value["progress"]) || (value["progress"] === null))) && (Object.hasOwn(value, "revision") && (typeof value["revision"] === "number" && Number.isSafeInteger(value["revision"]) && value["revision"] >= 0)) && (Object.hasOwn(value, "sync_warning") && (validSyncWarning(value["sync_warning"]) || (value["sync_warning"] === null)))); }
export function decodeWindowsRuntimeSnapshot(value: unknown): WindowsRuntimeSnapshot { if (!validWindowsRuntimeSnapshot(value)) throw new Error("Invalid WindowsRuntimeSnapshot response"); return value; }

export type WithheldRestoreFormat = {
  "code": string;
  "format": string;
};
function validWithheldRestoreFormat(value: unknown): value is WithheldRestoreFormat { return (isRecord(value) && (Object.hasOwn(value, "code") && (typeof value["code"] === "string")) && (Object.hasOwn(value, "format") && (typeof value["format"] === "string"))); }
export function decodeWithheldRestoreFormat(value: unknown): WithheldRestoreFormat { if (!validWithheldRestoreFormat(value)) throw new Error("Invalid WithheldRestoreFormat response"); return value; }
