import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useThumbnailCache } from "./useThumbnailCache";

vi.mock("../tailsyncClient", () => ({
  getImageData: vi.fn(),
}));

import { getImageData } from "../tailsyncClient";

const mockedGetImageData = vi.mocked(getImageData);

function thumbnail(id: number) {
  return {
    id,
    thumbnail_b64: `b64-${id}`,
    thumbnail_width: 2,
    thumbnail_height: 2,
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
      result.current.loadThumbnail(1);
    });
    await waitFor(() => expect(result.current.thumbnails.has(1)).toBe(true));
    expect(result.current.thumbnails.get(1)).toEqual({
      b64: "b64-1",
      width: 2,
      height: 2,
    });

    // A second request is deduplicated by the in-flight marker.
    await act(async () => {
      result.current.loadThumbnail(1);
    });
    expect(mockedGetImageData).toHaveBeenCalledTimes(1);
  });

  it("evicts the oldest entries beyond the cap", async () => {
    mockedGetImageData.mockImplementation(async (id: number) => thumbnail(id));
    const { result } = renderHook(() => useThumbnailCache(2));

    await act(async () => {
      result.current.loadThumbnail(1);
      result.current.loadThumbnail(2);
      result.current.loadThumbnail(3);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(2));
    expect(result.current.thumbnails.has(1)).toBe(false);
    expect(result.current.thumbnails.has(2)).toBe(true);
    expect(result.current.thumbnails.has(3)).toBe(true);
  });

  it("clear resets the cache and allows reloading", async () => {
    mockedGetImageData.mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));

    act(() => result.current.clear());
    expect(result.current.thumbnails.size).toBe(0);

    await act(async () => {
      result.current.loadThumbnail(1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
  });

  it("releases the in-flight marker on failure so retries work", async () => {
    mockedGetImageData.mockRejectedValueOnce(new Error("boom"));
    mockedGetImageData.mockResolvedValue(thumbnail(1));
    const { result } = renderHook(() => useThumbnailCache(4));

    await act(async () => {
      result.current.loadThumbnail(1);
    });
    await act(async () => {
      result.current.loadThumbnail(1);
    });
    await waitFor(() => expect(result.current.thumbnails.size).toBe(1));
    expect(mockedGetImageData).toHaveBeenCalledTimes(2);
  });
});
