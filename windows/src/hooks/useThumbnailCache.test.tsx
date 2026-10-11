import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { thumbnailKeyForEntry, useThumbnailCache } from "./useThumbnailCache";

vi.mock("../tailsyncClient", () => ({
  getImageData: vi.fn(),
}));

import { getImageData } from "../tailsyncClient";

const mockedGetImageData = vi.mocked(getImageData);

function key(id: number) {
  return `k${id}`;
}

function thumbnail(id: number) {
  return sizedThumbnail(id, 2, 2);
}

function sizedThumbnail(id: number, width: number, height: number) {
  return {
    id,
    thumbnail_b64: btoa(String.fromCharCode(...new Uint8Array(width * height * 4).fill(id))),
    thumbnail_width: width,
    thumbnail_height: height,
  };
}

describe("useThumbnailCache", () => {
  beforeEach(() => {
    mockedGetImageData.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  it("loads a thumbnail once and serves it from the cache", async () => {
    mockedGetImageData.mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await waitFor(() => expect(result.current.thumbnails.has(key(1))).toBe(true));
    expect(result.current.thumbnails.get(key(1))).toEqual({
      b64: btoa(String.fromCharCode(...new Uint8Array(16).fill(1))),
      width: 2,
      height: 2,
    });

    // A second request is deduplicated by the in-flight marker.
    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    expect(mockedGetImageData).toHaveBeenCalledTimes(1);
  });

  it.each([
    "content analysis busy",
    { schema_version: 1, code: "temporarily_busy", retryable: true, message_key: "untrusted", detail_class: "private" },
  ])("retries temporary admission pressure: %j", async (error) => {
    mockedGetImageData.mockRejectedValueOnce(error).mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));
    await act(async () => { await result.current.loadThumbnail(key(1), 1); });
    expect(result.current.thumbnails.has(key(1))).toBe(true);
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
  });

  it.each([
    "invalid content",
    { schema_version: 1, code: "internal_error", retryable: false, message_key: "untrusted", detail_class: "private" },
    { schema_version: 1, code: "future", retryable: true, message_key: "error.temporarily_busy", detail_class: "contention" },
  ])("does not retry or cache permanent and unknown errors: %j", async (error) => {
    mockedGetImageData.mockRejectedValue(error);
    const { result } = renderHook(() => useThumbnailCache(4));
    await act(async () => { await result.current.loadThumbnail(key(1), 1); });
    expect(result.current.thumbnails.size).toBe(0);
    expect(mockedGetImageData).toHaveBeenCalledTimes(1);
  });

  it("evicts the oldest entries beyond the cap and permits reloading retained entries", async () => {
    mockedGetImageData.mockImplementation(async (id: number) => thumbnail(id));
    const { result } = renderHook(() => useThumbnailCache(2));
    act(() => result.current.retain(new Set([key(1), key(2), key(3)])));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
      result.current.loadThumbnail(key(2), 2);
      result.current.loadThumbnail(key(3), 3);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(2));
    expect(result.current.thumbnails.has(key(1))).toBe(false);
    expect(result.current.thumbnails.has(key(2))).toBe(true);
    expect(result.current.thumbnails.has(key(3))).toBe(true);
    await act(async () => { await result.current.loadThumbnail(key(1), 1); });
    expect(mockedGetImageData).toHaveBeenCalledTimes(4);
    expect([...result.current.thumbnails.keys()]).toEqual([key(3), key(1)]);
  });

  it("evicts oldest thumbnails when their decoded pixels exceed the byte budget", async () => {
    mockedGetImageData.mockImplementation(async (id: number) => sizedThumbnail(id, 10, 10));
    const { result } = renderHook(() => useThumbnailCache(10, 3250));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
      result.current.loadThumbnail(key(2), 2);
      result.current.loadThumbnail(key(3), 3);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(2));
    expect([...result.current.thumbnails.keys()]).toEqual([key(2), key(3)]);
  });

  it("clear resets the cache and allows reloading", async () => {
    mockedGetImageData.mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));

    act(() => result.current.clear());
    expect(result.current.thumbnails.size).toBe(0);

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
  });

  it("retains only thumbnails used by the current page", async () => {
    mockedGetImageData.mockImplementation(async (id: number) => thumbnail(id));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
      result.current.loadThumbnail(key(2), 2);
      result.current.loadThumbnail(key(3), 3);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(3));

    act(() => result.current.retain(new Set([key(2)])));
    expect([...result.current.thumbnails.keys()]).toEqual([key(2)]);

    act(() => result.current.retain(new Set([key(1), key(2)])));
    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await waitFor(() => expect(result.current.thumbnails.has(key(1))).toBe(true));
    expect(mockedGetImageData).toHaveBeenCalledTimes(4);
  });

  it("releases the in-flight marker on failure so retries work", async () => {
    mockedGetImageData.mockRejectedValueOnce(new Error("boom"));
    mockedGetImageData.mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
  });

  it("does not let a superseded request clear a newer in-flight marker", async () => {
    const resolvers: Array<(value: ReturnType<typeof thumbnail>) => void> = [];
    mockedGetImageData.mockImplementation(
      () => new Promise((resolve) => { resolvers.push(resolve); }) as never,
    );
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    act(() => result.current.retain(new Set()));
    act(() => result.current.retain(new Set([key(1)])));
    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    expect(mockedGetImageData).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolvers[0](thumbnail(1));
    });
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.size).toBe(0);

    await act(async () => {
      result.current.loadThumbnail(key(1), 1);
    });
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);

    await act(async () => {
      resolvers[1](thumbnail(1));
    });
    await waitFor(() => expect(result.current.thumbnails.has(key(1))).toBe(true));
  });

  it("binds the cache key to identity so a new revision is not served stale pixels", () => {
    const base = {
      id: 7,
      data_hash: "h",
      classifier_version: 1,
      composite: {
        identity: {
          capture_id: "c1",
          content_id: "i1",
          archive_revision: 1,
          archive_digest: "d1",
        },
      },
    };
    const rearchived = {
      ...base,
      composite: {
        identity: { ...base.composite.identity, archive_revision: 2, archive_digest: "d2" },
      },
    };
    const reclassified = { ...base, classifier_version: 2 };
    expect(thumbnailKeyForEntry(base)).toBe(thumbnailKeyForEntry({ ...base }));
    expect(thumbnailKeyForEntry(base)).not.toBe(thumbnailKeyForEntry(rearchived));
    expect(thumbnailKeyForEntry(base)).not.toBe(thumbnailKeyForEntry(reclassified));
  });

  it("permits retry after an empty successful response", async () => {
    mockedGetImageData.mockResolvedValueOnce({ ...thumbnail(1), thumbnail_b64: "" });
    mockedGetImageData.mockResolvedValueOnce(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.has("entry")).toBe(true);
  });

  it("rejects malformed dimensions before cache admission", async () => {
    mockedGetImageData.mockResolvedValue({ ...thumbnail(1), thumbnail_width: Number.NaN });
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(result.current.thumbnails.size).toBe(0);
  });

  it("binds owner and backend authority and refuses a wrong response key", async () => {
    const entry = { id: 1, data_hash: "same", display: { cache_key: "th1:current" } };
    const key = thumbnailKeyForEntry(entry);
    expect(key).not.toBe(thumbnailKeyForEntry(entry, "favorites"));
    expect(key).not.toBe(thumbnailKeyForEntry({ ...entry, display: { cache_key: "th1:reopened" } }));
    mockedGetImageData.mockResolvedValue({ ...thumbnail(1), cache_key: "th1:old" });
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail(key, 1); });
    expect(mockedGetImageData).toHaveBeenCalledWith(1, "th1:current");
    expect(result.current.thumbnails.size).toBe(0);
    mockedGetImageData.mockResolvedValue({ ...thumbnail(1), cache_key: "th1:current" });
    await act(async () => { await result.current.loadThumbnail(key, 1); });
    expect(result.current.thumbnails.has(key)).toBe(true);
  });

  it("caps the real queue across rapid generation changes and drains cancelled jobs", async () => {
    let release: (value: ReturnType<typeof thumbnail>) => void = () => undefined;
    mockedGetImageData.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    mockedGetImageData.mockImplementation(async id => thumbnail(id));
    const { result } = renderHook(() => useThumbnailCache(2));
    const jobs: Promise<void>[] = [];
    act(() => { jobs.push(result.current.loadThumbnail("old", 1)); });
    await waitFor(() => expect(mockedGetImageData).toHaveBeenCalledTimes(1));
    for (let index = 0; index < 100; index += 1) {
      act(() => { result.current.clear(); jobs.push(result.current.loadThumbnail(`queued-${index}`, index + 2)); });
    }
    await act(async () => { release(thumbnail(1)); await Promise.all(jobs); });
    expect(mockedGetImageData).toHaveBeenCalledTimes(1);
    expect(result.current.thumbnails.size).toBe(0);
    await act(async () => { await result.current.loadThumbnail("latest", 200); });
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.has("latest")).toBe(true);
  });

  it("quarantines responses after unmount", async () => {
    let release: (value: ReturnType<typeof thumbnail>) => void = () => undefined;
    mockedGetImageData.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const { result, unmount } = renderHook(() => useThumbnailCache(2));
    let job: Promise<void> = Promise.resolve();
    act(() => { job = result.current.loadThumbnail("entry", 1); });
    await waitFor(() => expect(mockedGetImageData).toHaveBeenCalledTimes(1));
    unmount();
    await act(async () => { release(thumbnail(1)); await job; });
    expect(result.current.thumbnails.size).toBe(0);
  });
});
