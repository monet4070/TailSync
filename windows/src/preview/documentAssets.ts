import type * as Contract from "../types/localContracts.generated";
import {
  documentAssetBytes,
  type DecodedDocumentFrame,
} from "../utils/documentPreview";

export const DOCUMENT_FRONTEND_MAX_RETAINED_BYTES = 64 * 1024 * 1024;
export const DOCUMENT_FRONTEND_MAX_PLACEMENT_PIXELS = 16_000_000;

export type DocumentPrepareErrorCode = "too-large" | "invalid-asset" | "cancelled";

export class DocumentPrepareError extends Error {
  readonly code: DocumentPrepareErrorCode;

  constructor(code: DocumentPrepareErrorCode, message: string) {
    super(message);
    this.name = "DocumentPrepareError";
    this.code = code;
  }
}

export interface PreparedDocumentAssets {
  readonly frame: DecodedDocumentFrame;
  readonly urls: ReadonlyMap<number, string>;
  readonly retainedBytes: number;
  readonly imagePlan: DocumentImagePlan;
  release(): void;
}

export interface DocumentImagePlacement {
  readonly kind: "image";
  readonly assetIndex: number;
  readonly url: string;
}

export interface DocumentImagePlaceholder {
  readonly kind: "placeholder";
  readonly availability: Contract.DocumentImageAvailability;
  readonly budgetLimited: boolean;
}

export type DocumentImagePlanEntry = DocumentImagePlacement | DocumentImagePlaceholder;

export interface DocumentImagePlan {
  readonly placements: ReadonlyMap<number, DocumentImagePlanEntry>;
  readonly pixelBudgetExceeded: boolean;
}

function frameRetainedBytes(frame: DecodedDocumentFrame): number {
  return frame.assets.buffer.byteLength + frame.metadata.model_bytes * 8
    + frame.document.assets.reduce((total, asset) => total + asset.length * 2, 0)
    + 4 * 1024 * 1024;
}

function createUrl(bytes: Uint8Array<ArrayBuffer>): string {
  return URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
}

function revokeUrl(url: string): void {
  URL.revokeObjectURL(url);
}

export async function prepareDocumentAssets(
  frame: DecodedDocumentFrame,
  signal?: AbortSignal,
  limits: { maxRetainedBytes?: number } = {},
): Promise<PreparedDocumentAssets> {
  if (signal?.aborted) throw new DocumentPrepareError("cancelled", "Document preparation cancelled");
  const deadline = performance.now() + 2000;
  const checkWork = () => {
    if (signal?.aborted) throw new DocumentPrepareError("cancelled", "Document preparation cancelled");
    if (performance.now() > deadline) throw { code: "payload_unavailable", retryable: true };
  };
  const maxRetainedBytes = limits.maxRetainedBytes ?? DOCUMENT_FRONTEND_MAX_RETAINED_BYTES;
  let retainedBytes = frameRetainedBytes(frame);
  if (retainedBytes > maxRetainedBytes) {
    throw new DocumentPrepareError("too-large", "Document frame exceeds the renderer budget");
  }
  const urls = new Map<number, string>();
  const provisional = new Map(frame.document.assets.map((asset) => [asset.index, "pending"]));
  const pixelLimit = Math.min(DOCUMENT_FRONTEND_MAX_PLACEMENT_PIXELS, Math.floor((maxRetainedBytes - retainedBytes) / 4));
  const admission = planDocumentImages(frame.document, provisional, pixelLimit);
  const admittedAssets = new Set<number>();
  for (const [blockIndex, placement] of admission.placements) {
    if (placement.kind !== "image") continue;
    admittedAssets.add(placement.assetIndex);
    const asset = frame.document.assets[placement.assetIndex];
    retainedBytes += asset.width * asset.height * 4;
    if (frame.document.blocks[blockIndex].kind !== "image") throw new DocumentPrepareError("invalid-asset", "Invalid image placement");
  }
  const created: string[] = [];
  const release = () => {
    for (const url of created) revokeUrl(url);
    created.length = 0;
    urls.clear();
  };
  try {
    for (const asset of frame.document.assets) {
      checkWork();
      const bytes = await documentAssetBytes(frame, asset);
      checkWork();
      if (bytes === null) {
        throw new DocumentPrepareError("invalid-asset", "Document asset failed validation");
      }
      if (!admittedAssets.has(asset.index)) continue;
      const url = createUrl(bytes);
      created.push(url);
      urls.set(asset.index, url);
    }
    checkWork();
    const placements = new Map<number, DocumentImagePlanEntry>();
    for (const [blockIndex, placement] of admission.placements) {
      placements.set(blockIndex, placement.kind === "image"
        ? { ...placement, url: urls.get(placement.assetIndex)! }
        : placement);
    }
    const imagePlan = { placements, pixelBudgetExceeded: admission.pixelBudgetExceeded };
    return { frame, urls, retainedBytes, imagePlan, release };
  } catch (error) {
    release();
    throw error;
  }
}

export function planDocumentImages(
  document: Contract.DisplayDocument,
  urls: ReadonlyMap<number, string>,
  maxPlacementPixels: number = DOCUMENT_FRONTEND_MAX_PLACEMENT_PIXELS,
): DocumentImagePlan {
  const placements = new Map<number, DocumentImagePlanEntry>();
  let pixels = 0;
  let pixelBudgetExceeded = false;
  document.blocks.forEach((block, index) => {
    if (block.kind !== "image" || !block.image) return;
    const image = block.image;
    const assetIndex = image.asset ?? null;
    const asset = assetIndex === null ? null : document.assets[assetIndex] ?? null;
    const url = assetIndex === null ? undefined : urls.get(assetIndex);
    if (image.availability !== "available" || assetIndex === null || asset === null || url === undefined) {
      placements.set(index, {
        kind: "placeholder",
        availability: image.availability,
        budgetLimited: false,
      });
      return;
    }
    const cost = asset.width * asset.height;
    if (!Number.isSafeInteger(cost) || pixels + cost > maxPlacementPixels) {
      pixelBudgetExceeded = true;
      placements.set(index, {
        kind: "placeholder",
        availability: image.availability,
        budgetLimited: true,
      });
      return;
    }
    pixels += cost;
    placements.set(index, { kind: "image", assetIndex, url });
  });
  return { placements, pixelBudgetExceeded };
}

export interface DocumentBlockGroupItem {
  readonly blockIndex: number;
  readonly block: Contract.DocumentBlock;
}

export type DocumentBlockGroup =
  | { readonly kind: "paragraph"; readonly blockIndex: number; readonly block: Contract.DocumentBlock }
  | {
    readonly kind: "list";
    readonly list: Contract.DocumentListKind;
    readonly depth: number;
    readonly quoteDepth: number;
    readonly code: boolean;
    readonly start: number;
    readonly items: ReadonlyArray<DocumentBlockGroupItem>;
  }
  | {
    readonly kind: "quote";
    readonly depth: number;
    readonly code: boolean;
    readonly items: ReadonlyArray<DocumentBlockGroupItem>;
  }
  | { readonly kind: "image"; readonly blockIndex: number; readonly block: Contract.DocumentBlock }
  | { readonly kind: "table"; readonly blockIndex: number; readonly block: Contract.DocumentBlock };

type PendingGroup =
  | {
    readonly kind: "list";
    readonly key: string;
    readonly list: Contract.DocumentListKind;
    readonly depth: number;
    readonly quoteDepth: number;
    readonly code: boolean;
    readonly start: number;
    readonly items: DocumentBlockGroupItem[];
  }
  | {
    readonly kind: "quote";
    readonly key: string;
    readonly depth: number;
    readonly code: boolean;
    readonly items: DocumentBlockGroupItem[];
  };

export function groupDocumentBlocks(blocks: ReadonlyArray<Contract.DocumentBlock>): DocumentBlockGroup[] {
  const groups: DocumentBlockGroup[] = [];
  const counters = new Map<number, { key: string; count: number }>();
  let pending: PendingGroup | null = null;
  const flush = () => {
    if (!pending) return;
    if (pending.kind === "list") {
      groups.push({
        kind: "list",
        list: pending.list,
        depth: pending.depth,
        quoteDepth: pending.quoteDepth,
        code: pending.code,
        start: pending.start,
        items: pending.items,
      });
    } else {
      groups.push({ kind: "quote", depth: pending.depth, code: pending.code, items: pending.items });
    }
    pending = null;
  };
  blocks.forEach((block, blockIndex) => {
    const paragraph = block.kind === "paragraph" ? block.paragraph ?? null : null;
    const list = paragraph?.list ?? null;
    const depth = paragraph?.list_depth ?? 0;
    const quoteDepth = paragraph?.quote_depth ?? 0;
    const code = paragraph?.code_block ?? false;
    if (paragraph && list !== null) {
      const key = `list:${list}:${depth}:${quoteDepth}:${code ? "code" : "text"}`;
      for (const nestedDepth of counters.keys()) {
        if (nestedDepth > depth) counters.delete(nestedDepth);
      }
      const previous = counters.get(depth);
      const count = list === "ordered" ? (previous?.key === key ? previous.count : 0) + 1 : 0;
      counters.set(depth, { key, count });
      if (pending?.kind === "list" && pending.key === key) {
        pending.items.push({ blockIndex, block });
        return;
      }
      flush();
      pending = { kind: "list", key, list, depth, quoteDepth, code, start: Math.max(1, count), items: [{ blockIndex, block }] };
      return;
    }
    counters.clear();
    if (paragraph && quoteDepth > 0) {
      const key = `quote:${quoteDepth}:${code ? "code" : "text"}`;
      if (pending?.kind === "quote" && pending.key === key) {
        pending.items.push({ blockIndex, block });
        return;
      }
      flush();
      pending = { kind: "quote", key, depth: quoteDepth, code, items: [{ blockIndex, block }] };
      return;
    }
    flush();
    groups.push({ kind: block.kind, blockIndex, block });
  });
  flush();
  return groups;
}
