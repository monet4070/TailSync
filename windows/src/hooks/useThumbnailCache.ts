// Thumbnail cache for the history list (T254 extraction from History.tsx).
//
// Loads image thumbnails on demand through the typed client, deduplicates
// in-flight loads, and evicts the oldest entries (Map insertion order)
// beyond `maxEntries`. `clear()` resets both the cache and the in-flight
// markers so entries can be reloaded (e.g. after clearing history).

import { useCallback, useRef, useState } from "react";
import { getImageData } from "../tailsyncClient";

export interface ThumbnailData {
  b64: string;
  width: number;
  height: number;
}

export function useThumbnailCache(maxEntries: number) {
  const [thumbnails, setThumbnails] = useState<Map<number, ThumbnailData>>(new Map());
  const inFlight = useRef<Set<number>>(new Set());
  const retainedIds = useRef<Set<number> | null>(null);
  const generation = useRef(0);

  const loadThumbnail = useCallback(
    async (id: number) => {
      if (inFlight.current.has(id)) return;
      inFlight.current.add(id);
      const requestGeneration = generation.current;
      try {
        const resp = await getImageData(id);
        if (
          requestGeneration !== generation.current ||
          (retainedIds.current !== null && !retainedIds.current.has(id))
        ) {
          inFlight.current.delete(id);
          return;
        }
        if (resp.thumbnail_b64) {
          setThumbnails((current) => {
            const next = new Map(current);
            next.delete(id);
            next.set(id, {
              b64: resp.thumbnail_b64,
              width: resp.thumbnail_width,
              height: resp.thumbnail_height,
            });
            while (next.size > maxEntries) {
              const oldestId = next.keys().next().value;
              if (oldestId === undefined) break;
              next.delete(oldestId);
              inFlight.current.delete(oldestId);
            }
            return next;
          });
        }
      } catch (error) {
        inFlight.current.delete(id);
        console.error(`Thumbnail load failed for ${id}:`, error);
      }
    },
    [maxEntries],
  );

  const retain = useCallback((ids: Set<number>) => {
    retainedIds.current = new Set(ids);
    for (const id of inFlight.current) {
      if (!ids.has(id)) inFlight.current.delete(id);
    }
    setThumbnails((current) => {
      if ([...current.keys()].every((id) => ids.has(id))) return current;
      return new Map([...current].filter(([id]) => ids.has(id)));
    });
  }, []);

  const clear = useCallback(() => {
    generation.current += 1;
    retainedIds.current = null;
    inFlight.current.clear();
    setThumbnails(new Map());
  }, []);

  return { thumbnails, loadThumbnail, retain, clear };
}
