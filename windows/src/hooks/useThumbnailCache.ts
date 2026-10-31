// Thumbnail cache for the history list (T254 extraction from History.tsx).
//
// Loads image thumbnails on demand through the typed client, deduplicates
// in-flight loads, and evicts the oldest entries (Map insertion order)
// beyond either the entry or byte budget. `clear()` resets both the cache
// and the in-flight markers so entries can be reloaded (e.g. after clearing
// history).

import { useCallback, useEffect, useRef, useState } from "react";
import { getImageData } from "../tailsyncClient";
import { decodeStableErrorEnvelope } from "../types/localContracts.generated";
import { StableIpcError } from "../utils/stableIpcError";
import { isDemoEntry } from "../onboarding/demo";

export interface ThumbnailData {
  b64: string;
  width: number;
  height: number;
}

export const THUMBNAIL_SPEC = 48;

export interface ThumbnailKeyParts {
  id: number;
  dataHash: string;
  captureId: string;
  contentId: string;
  archiveRevision: number;
  archiveDigest: string;
  classifierVersion: number;
  spec: number;
}

export interface ThumbnailEntryLike {
  id: number;
  data_hash: string;
  classifier_version?: number;
  display?: { cache_key: string } | null;
  composite?: {
    identity?: {
      capture_id: string;
      content_id: string;
      archive_revision: number;
      archive_digest: string;
    } | null;
  } | null;
}

export function thumbnailCacheKey(parts: ThumbnailKeyParts): string {
  return [
    parts.id,
    parts.dataHash,
    parts.captureId,
    parts.contentId,
    parts.archiveRevision,
    parts.archiveDigest,
    parts.classifierVersion,
    parts.spec,
  ].join("|");
}

export function thumbnailKeyForEntry(entry: ThumbnailEntryLike, owner = "all"): string {
  if (entry.display?.cache_key) return JSON.stringify([owner, entry.display.cache_key, THUMBNAIL_SPEC]);
  const identity = entry.composite?.identity;
  return thumbnailCacheKey({
    id: entry.id,
    dataHash: entry.data_hash,
    captureId: identity?.capture_id ?? "",
    contentId: identity?.content_id ?? "",
    archiveRevision: identity?.archive_revision ?? 0,
    archiveDigest: identity?.archive_digest ?? "",
    classifierVersion: entry.classifier_version ?? 0,
    spec: THUMBNAIL_SPEC,
  }) + `|${owner}`;
}

function thumbnailMemoryCost(thumbnail: ThumbnailData): number {
  const decodedPixels = Math.max(0, thumbnail.width) * Math.max(0, thumbnail.height) * 4;
  const encodedString = thumbnail.b64.length * 2;
  return Math.min(Number.MAX_SAFE_INTEGER, decodedPixels + encodedString);
}

function thumbnailCacheCost(thumbnails: Map<string, ThumbnailData>): number {
  let total = 0;
  for (const thumbnail of thumbnails.values()) {
    total = Math.min(Number.MAX_SAFE_INTEGER, total + thumbnailMemoryCost(thumbnail));
  }
  return total;
}

function backendKey(key: string): string | undefined {
  try {
    const parts: unknown = JSON.parse(key);
    if (Array.isArray(parts) && parts.length === 3 && typeof parts[1] === "string") return parts[1];
  } catch {}
  return undefined;
}

function validThumbnail(thumbnail: ThumbnailData): boolean {
  const pixels = thumbnail.width * thumbnail.height;
  if (!Number.isSafeInteger(pixels) || !Number.isSafeInteger(thumbnail.width) ||
    !Number.isSafeInteger(thumbnail.height) || thumbnail.width <= 0 || thumbnail.height <= 0 ||
    thumbnail.width > 256 || thumbnail.height > 256 || thumbnail.b64.length > 350_000) return false;
  try { return atob(thumbnail.b64).length === pixels * 4; } catch { return false; }
}

// The IPC boundary normalizes versioned envelopes to `StableIpcError`, so the
// normalized code is the production shape; raw envelopes and legacy busy text
// stay accepted for the un-normalized callers.
function temporaryPressure(error: unknown): boolean {
  if (error instanceof StableIpcError) return error.code === "temporarily_busy";
  try { return decodeStableErrorEnvelope(error).code === "temporarily_busy"; } catch {
    return String(error) === "content analysis busy" || String(error).endsWith(": content analysis busy");
  }
}

export function useThumbnailCache(maxEntries: number, maxBytes = 4 * 1024 * 1024) {
  const [thumbnails, setThumbnails] = useState<Map<string, ThumbnailData>>(new Map());
  const cache = useRef(new Map<string, ThumbnailData>());
  const inFlight = useRef(new Map<string, number>());
  const nextRequest = useRef(0);
  const retainedKeys = useRef<Set<string> | null>(null);
  const generation = useRef(0);
  const active = useRef(true);
  const queue = useRef(Promise.resolve());
  const queuedCount = useRef(0);

  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      generation.current += 1;
      inFlight.current.clear();
      cache.current.clear();
    };
  }, []);

  const loadThumbnail = useCallback(async (key: string, id: number) => {
    // Demo previews have bundled bytes; their virtual IDs have no DB thumbnail.
    if (isDemoEntry(id)) return;
    if (!active.current || cache.current.has(key) || inFlight.current.has(key) || queuedCount.current >= 64) return;
    queuedCount.current += 1;
    const requestId = ++nextRequest.current;
    const requestGeneration = generation.current;
    inFlight.current.set(key, requestId);
    const current = () => active.current && generation.current === requestGeneration &&
      inFlight.current.get(key) === requestId &&
      (retainedKeys.current === null || retainedKeys.current.has(key));
    const work = queue.current.then(async () => {
      try {
        if (!current()) return;
        const expected = backendKey(key);
        const response = await (async () => {
          for (let attempt = 0; attempt < 16; attempt += 1) {
            if (!current()) throw new Error("thumbnail request superseded");
            try { return await getImageData(id, expected); } catch (error) {
              if (!temporaryPressure(error)) throw error;
              if (attempt === 15) throw error;
              await new Promise(resolve => setTimeout(resolve, Math.min(400, 50 * (attempt + 1))));
            }
          }
          throw new Error("content analysis busy");
        })();
        if (!current() || (expected !== undefined && response.cache_key !== expected)) return;
        const thumbnail = { b64: response.thumbnail_b64, width: response.thumbnail_width, height: response.thumbnail_height };
        if (!validThumbnail(thumbnail) || thumbnailMemoryCost(thumbnail) > maxBytes || maxEntries <= 0) return;
        cache.current.delete(key);
        cache.current.set(key, thumbnail);
        while (cache.current.size > maxEntries || thumbnailCacheCost(cache.current) > maxBytes) {
          const oldest = cache.current.keys().next().value;
          if (oldest === undefined) break;
          cache.current.delete(oldest);
        }
        if (current()) setThumbnails(new Map(cache.current));
      } catch (error) {
        if (current()) console.error(`Thumbnail load failed for ${id}:`, error);
      } finally {
        queuedCount.current -= 1;
        if (inFlight.current.get(key) === requestId) inFlight.current.delete(key);
      }
    });
    queue.current = work;
    await work;
  }, [maxBytes, maxEntries]);

  const retain = useCallback((keys: Set<string>) => {
    retainedKeys.current = new Set(keys);
    for (const key of inFlight.current.keys()) if (!keys.has(key)) inFlight.current.delete(key);
    for (const key of cache.current.keys()) if (!keys.has(key)) cache.current.delete(key);
    setThumbnails(new Map(cache.current));
  }, []);

  const clear = useCallback(() => {
    generation.current += 1;
    retainedKeys.current = null;
    inFlight.current.clear();
    cache.current.clear();
    setThumbnails(new Map());
  }, []);

  return { thumbnails, loadThumbnail, retain, clear };
}
