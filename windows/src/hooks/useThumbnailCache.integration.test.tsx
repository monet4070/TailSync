// F4 regression: the thumbnail cache must consume the *normalized* error that
// the shared IPC boundary produces, not the pre-normalization envelope.
//
// Only `@tauri-apps/api/core` is mocked here. `useThumbnailCache`,
// `tailsyncClient.getImageData`, `diagnostics/localIpcMetrics.invoke`, and
// `normalizeIpcError` all run for real, so a versioned rejection travels the
// production path from the raw Tauri promise to `temporaryPressure`.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { tauriInvokeMock } = vi.hoisted(() => ({ tauriInvokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvokeMock }));

import { StableIpcError } from "../utils/stableIpcError";
import { getLocalIpcMetrics } from "../diagnostics/localIpcMetrics";
import { thumbnailKeyForEntry, useThumbnailCache } from "./useThumbnailCache";

interface Envelope {
  schema_version: number;
  code: string;
  retryable: boolean;
  message_key: string;
  detail_class: string;
}

function envelope(code: string, overrides: Partial<Envelope> = {}): Envelope {
  return {
    schema_version: 1,
    code,
    retryable: true,
    message_key: "untrusted.message",
    detail_class: "private",
    ...overrides,
  };
}

function thumbnail(id: number) {
  return {
    id,
    thumbnail_b64: btoa(String.fromCharCode(...new Uint8Array(16).fill(id))),
    thumbnail_width: 2,
    thumbnail_height: 2,
  };
}

type Hook = { current: ReturnType<typeof useThumbnailCache> };

/** Runs a load to completion, letting the bounded backoff timers fire. */
async function load(hook: Hook, key: string, id: number, budgetMs = 30_000) {
  let job: Promise<void> = Promise.resolve();
  await act(async () => {
    job = hook.current.loadThumbnail(key, id);
    await vi.advanceTimersByTimeAsync(budgetMs);
  });
  await job;
}

/**
 * Lets the queued job reach its first IPC call without firing backoff timers.
 * The job is wrapped so awaiting this helper does not adopt the job promise.
 */
async function startPending(hook: Hook, key: string, id: number): Promise<{ job: Promise<void> }> {
  let job: Promise<void> = Promise.resolve();
  await act(async () => {
    job = hook.current.loadThumbnail(key, id);
    await vi.advanceTimersByTimeAsync(10);
  });
  return { job };
}

/** Fires the pending backoff timers without awaiting the still-running job. */
async function advanceBackoff(budgetMs = 30_000) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(budgetMs);
  });
}

const consoleErrors: unknown[][] = [];

const RETRY_CASES: Array<[string, Envelope]> = [
  ["a versioned temporarily_busy envelope", envelope("temporarily_busy")],
  [
    "an envelope whose untrusted fields contradict the fixed policy",
    envelope("temporarily_busy", {
      retryable: false,
      message_key: "error.internal",
      detail_class: "internal",
    }),
  ],
];

const NO_RETRY_CASES: Array<[string, Envelope, string]> = [
  ["storage_unavailable", envelope("storage_unavailable"), "storage_unavailable"],
  ["internal_error", envelope("internal_error", { retryable: false }), "internal_error"],
  ["an unknown future code", envelope("future_error"), "internal_error"],
];

describe("useThumbnailCache over the real IPC boundary", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    consoleErrors.length = 0;
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      consoleErrors.push(args);
    });
    vi.stubEnv("VITE_TAILSYNC_DIAGNOSTICS", "0");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllEnvs();
  });

  it.each(RETRY_CASES)("retries %s and caches the thumbnail", async (_label, error) => {
    tauriInvokeMock.mockRejectedValueOnce(error).mockResolvedValueOnce(thumbnail(1));
    const hook = renderHook(() => useThumbnailCache(4));
    await load(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(2);
    expect(tauriInvokeMock).toHaveBeenNthCalledWith(1, "get_image_data", { id: 1 });
    expect(tauriInvokeMock).toHaveBeenNthCalledWith(2, "get_image_data", { id: 1 });
    expect(hook.result.current.thumbnails.has("k1")).toBe(true);
  });

  it.each(NO_RETRY_CASES)("does not retry %s and leaves the cache empty", async (_label, error, code) => {
    tauriInvokeMock.mockRejectedValue(error);
    const hook = renderHook(() => useThumbnailCache(4));
    await load(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    expect(hook.result.current.thumbnails.size).toBe(0);
    // Proves the rejection really crossed `normalizeIpcError` in this test.
    expect(consoleErrors).toHaveLength(1);
    expect(consoleErrors[0]?.[1]).toBeInstanceOf(StableIpcError);
    expect(consoleErrors[0]?.[1]).toMatchObject({ code });
  });

  it("retries the normalized busy code with diagnostics enabled and a Chinese locale", async () => {
    vi.stubEnv("VITE_TAILSYNC_DIAGNOSTICS", "1");
    const previousLocale = localStorage.getItem("tailsync-lang");
    localStorage.setItem("tailsync-lang", "zh-CN");
    try {
      tauriInvokeMock.mockRejectedValueOnce(envelope("temporarily_busy")).mockResolvedValueOnce(thumbnail(1));
      const hook = renderHook(() => useThumbnailCache(4));
      await load(hook.result, "k1", 1);
      expect(tauriInvokeMock).toHaveBeenCalledTimes(2);
      expect(hook.result.current.thumbnails.has("k1")).toBe(true);
      expect(getLocalIpcMetrics()?.commands).toContainEqual(expect.objectContaining({
        command: "get_image_data", count: 2, failures: 1,
      }));
    } finally {
      if (previousLocale === null) localStorage.removeItem("tailsync-lang");
      else localStorage.setItem("tailsync-lang", previousLocale);
    }
  });

  it.each([["content analysis busy"], ["get_image_data: content analysis busy"]])(
    "still retries the legacy busy text %s",
    async (error) => {
      tauriInvokeMock.mockRejectedValueOnce(error).mockResolvedValueOnce(thumbnail(1));
      const hook = renderHook(() => useThumbnailCache(4));
      await load(hook.result, "k1", 1);
      expect(tauriInvokeMock).toHaveBeenCalledTimes(2);
      expect(hook.result.current.thumbnails.has("k1")).toBe(true);
    },
  );

  it("bounds the retry budget at sixteen attempts", async () => {
    tauriInvokeMock.mockRejectedValue(envelope("temporarily_busy"));
    const hook = renderHook(() => useThumbnailCache(4));
    await load(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(16);
    expect(hook.result.current.thumbnails.size).toBe(0);
    expect(consoleErrors).toHaveLength(1);
  });

  it("passes the display cache key through the real client and refuses a stale backend key", async () => {
    const key = thumbnailKeyForEntry({ id: 7, data_hash: "h", display: { cache_key: "th1:current" } });
    tauriInvokeMock.mockResolvedValueOnce({ ...thumbnail(7), cache_key: "th1:stale" });
    const hook = renderHook(() => useThumbnailCache(4));
    await load(hook.result, key, 7);
    expect(tauriInvokeMock).toHaveBeenCalledWith("get_image_data", { id: 7, cacheKey: "th1:current" });
    expect(hook.result.current.thumbnails.size).toBe(0);
  });

  it("stops retrying once the hook is unmounted", async () => {
    tauriInvokeMock.mockRejectedValue(envelope("temporarily_busy"));
    const hook = renderHook(() => useThumbnailCache(4));
    const { job } = await startPending(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    hook.unmount();
    await advanceBackoff();
    await job;
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    expect(hook.result.current.thumbnails.size).toBe(0);
  });

  it("stops retrying once the cache is cleared", async () => {
    tauriInvokeMock.mockRejectedValue(envelope("temporarily_busy"));
    const hook = renderHook(() => useThumbnailCache(4));
    const { job } = await startPending(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    act(() => hook.result.current.clear());
    await advanceBackoff();
    await job;
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    expect(hook.result.current.thumbnails.size).toBe(0);
  });

  it("stops retrying once the key is no longer retained", async () => {
    tauriInvokeMock.mockRejectedValue(envelope("temporarily_busy"));
    const hook = renderHook(() => useThumbnailCache(4));
    const { job } = await startPending(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    act(() => hook.result.current.retain(new Set<string>()));
    await advanceBackoff();
    await job;
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    expect(hook.result.current.thumbnails.size).toBe(0);
  });

  it("does not cache a thumbnail that arrives after the cache was cleared", async () => {
    let release: (value: unknown) => void = () => undefined;
    tauriInvokeMock.mockImplementation(() => new Promise((resolve) => { release = resolve; }));
    const hook = renderHook(() => useThumbnailCache(4));
    const { job } = await startPending(hook.result, "k1", 1);
    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    act(() => hook.result.current.clear());
    await act(async () => {
      release(thumbnail(1));
    });
    await job;
    expect(hook.result.current.thumbnails.size).toBe(0);
    expect(consoleErrors).toHaveLength(0);
  });
});
