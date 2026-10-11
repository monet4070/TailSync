import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { thumbnailKeyForEntry, useThumbnailCache } from "./useThumbnailCache";

vi.mock("../tailsyncClient", () => ({ getImageData: vi.fn() }));
import { getImageData } from "../tailsyncClient";

const request = vi.mocked(getImageData);
const image = (id: number) => ({ id, thumbnail_b64: "AAAA/w==", thumbnail_width: 1, thumbnail_height: 1 });

describe("parent independent thumbnail regressions", () => {
  beforeEach(() => {
    request.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  it("permits reloading an evicted but retained thumbnail", async () => {
    request.mockImplementation(async id => image(id));
    const { result } = renderHook(() => useThumbnailCache(1));
    act(() => result.current.retain(new Set(["first", "second"])));
    await act(async () => { await result.current.loadThumbnail("first", 1); });
    await act(async () => { await result.current.loadThumbnail("second", 2); });
    await waitFor(() => expect(result.current.thumbnails.has("first")).toBe(false));
    await act(async () => { await result.current.loadThumbnail("first", 1); });
    expect(request).toHaveBeenCalledTimes(3);
    await waitFor(() => expect(result.current.thumbnails.has("first")).toBe(true));
  });

  it("permits retry after an empty successful response", async () => {
    request.mockResolvedValueOnce({ ...image(1), thumbnail_b64: "" });
    request.mockResolvedValueOnce(image(1));
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(request).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(result.current.thumbnails.has("entry")).toBe(true));
  });

  it("rejects malformed dimensions before cache admission", async () => {
    request.mockResolvedValue({ ...image(1), thumbnail_width: Number.NaN });
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(result.current.thumbnails.size).toBe(0);
  });

  it("binds owner and backend authority and refuses a wrong response key", async () => {
    const entry = { id: 1, data_hash: "same", display: { cache_key: "th1:current" } };
    const key = thumbnailKeyForEntry(entry);
    expect(key).not.toBe(thumbnailKeyForEntry(entry, "favorites"));
    expect(key).not.toBe(thumbnailKeyForEntry({ ...entry, display: { cache_key: "th1:reopened" } }));
    request.mockResolvedValue({ ...image(1), cache_key: "th1:old" });
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail(key, 1); });
    expect(request).toHaveBeenCalledWith(1, "th1:current");
    expect(result.current.thumbnails.size).toBe(0);
    request.mockResolvedValue({ ...image(1), cache_key: "th1:current" });
    await act(async () => { await result.current.loadThumbnail(key, 1); });
    expect(result.current.thumbnails.has(key)).toBe(true);
  });

  it("retries temporary admission pressure without retrying invalid content", async () => {
    request.mockRejectedValueOnce("content analysis busy").mockResolvedValueOnce(image(1));
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(request).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.has("entry")).toBe(true);
    request.mockRejectedValueOnce("invalid content");
    await act(async () => { await result.current.loadThumbnail("invalid", 2); });
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("retries the actual fixed-policy Tauri envelope but rejects unknown error codes", async () => {
    const envelope = { schema_version: 1, code: "temporarily_busy", retryable: true, message_key: "error.temporarily_busy", detail_class: "contention" };
    request.mockRejectedValueOnce(envelope).mockResolvedValueOnce(image(1));
    const { result } = renderHook(() => useThumbnailCache(2));
    await act(async () => { await result.current.loadThumbnail("entry", 1); });
    expect(request).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.has("entry")).toBe(true);
    request.mockRejectedValueOnce({ ...envelope, code: "future", retryable: true });
    await act(async () => { await result.current.loadThumbnail("invalid", 2); });
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("caps the real queue across rapid generation changes and drains cancelled jobs", async () => {
    let release: (value: ReturnType<typeof image>) => void = () => undefined;
    request.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    request.mockImplementation(async id => image(id));
    const { result } = renderHook(() => useThumbnailCache(2));
    const jobs: Promise<void>[] = [];
    act(() => { jobs.push(result.current.loadThumbnail("old", 1)); });
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    for (let index = 0; index < 100; index += 1) {
      act(() => { result.current.clear(); jobs.push(result.current.loadThumbnail(`queued-${index}`, index + 2)); });
    }
    await act(async () => { release(image(1)); await Promise.all(jobs); });
    expect(request).toHaveBeenCalledTimes(1);
    expect(result.current.thumbnails.size).toBe(0);
    await act(async () => { await result.current.loadThumbnail("latest", 200); });
    expect(request).toHaveBeenCalledTimes(2);
    expect(result.current.thumbnails.has("latest")).toBe(true);
  });

  it("quarantines responses after unmount", async () => {
    let release: (value: ReturnType<typeof image>) => void = () => undefined;
    request.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const { result, unmount } = renderHook(() => useThumbnailCache(2));
    let job: Promise<void> = Promise.resolve();
    act(() => { job = result.current.loadThumbnail("entry", 1); });
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    unmount();
    await act(async () => { release(image(1)); await job; });
    expect(result.current.thumbnails.size).toBe(0);
  });
});
