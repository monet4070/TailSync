import { createElement, StrictMode, useLayoutEffect } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildPngAsset,
  concatAssets,
  documentAsset,
  documentImageBlock,
  encodeDocumentFrame,
} from "../test/documentFrames";
import { DocumentParseError, decodeDocumentFrame, type DecodedDocumentFrame } from "../utils/documentPreview";
import type { DocumentTarget } from "./useDocumentPreview";
import { useDocumentPreview } from "./useDocumentPreview";

const mocks = vi.hoisted(() => ({
  getDocumentPreview: vi.fn(),
  validateDocumentPreview: vi.fn(),
  cancelDocumentPreview: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../tailsyncClient", () => mocks);

interface DocumentRequestLike {
  entry_id: number;
  owner: "history" | "favorites";
  generation: number;
  request_id: string;
}

function buildFrame(request: DocumentRequestLike): DecodedDocumentFrame {
  const asset = documentAsset(0, 0, buildPngAsset(2, 2, 0x11));
  const document = {
    version: 1,
    blocks: [documentImageBlock(0, 0)],
    assets: [asset.asset],
    partial: [],
    styles_partial: false,
    first_image: 0,
  };
  return decodeDocumentFrame(
    encodeDocumentFrame(
      { entry_id: request.entry_id, owner: request.owner, generation: request.generation, request_id: request.request_id },
      document,
      concatAssets([asset]),
    ),
    request,
  );
}

function requestAt(index: number): DocumentRequestLike {
  return mocks.getDocumentPreview.mock.calls[index][0] as DocumentRequestLike;
}

const historyTarget: DocumentTarget = { entryId: 7, owner: "history" };

describe("useDocumentPreview", () => {
  beforeEach(() => {
    localStorage.clear();
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:prepared-${counter++}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    mocks.getDocumentPreview.mockImplementation(async (request: DocumentRequestLike) => buildFrame(request));
    mocks.validateDocumentPreview.mockResolvedValue(true);
  });

  afterEach(() => vi.restoreAllMocks());

  it("prepares and releases bundled PPTX content without database or cancellation IPC", async () => {
    localStorage.setItem("tailsync-guided-tour-v2", JSON.stringify({ active: true, index: 8 }));
    const hook = renderHook(({ target }: { target: DocumentTarget | null }) => useDocumentPreview(target), {
      initialProps: { target: { entryId: 8_000_000_012, owner: "history" } as DocumentTarget | null },
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    if (hook.result.current.state.status !== "ready") throw new Error("PPTX demo not ready");
    expect(hook.result.current.state.prepared.frame.metadata.entry_id).toBe(8_000_000_012);
    const release = vi.spyOn(hook.result.current.state.prepared, "release");
    hook.rerender({ target: null });
    expect(release).toHaveBeenCalled();
    expect(mocks.getDocumentPreview).not.toHaveBeenCalled();
    expect(mocks.validateDocumentPreview).not.toHaveBeenCalled();
    expect(mocks.cancelDocumentPreview).not.toHaveBeenCalled();
  });

  it("keeps generations monotonic across remount, owner change and retry", async () => {
    const first = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(first.result.current.state.status).toBe("ready"));
    const firstGeneration = requestAt(0).generation;
    expect(Number.isSafeInteger(firstGeneration)).toBe(true);
    expect(firstGeneration).toBeLessThanOrEqual(Number.MAX_SAFE_INTEGER);
    first.unmount();

    const second = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(second.result.current.state.status).toBe("ready"));
    const secondGeneration = requestAt(1).generation;
    expect(secondGeneration).toBeGreaterThan(firstGeneration);

    act(() => second.result.current.retry());
    await waitFor(() => expect(requestAt(2).generation).toBeGreaterThan(secondGeneration));
    second.unmount();
  });

  it("replaces state and releases the previous object URLs when the owner changes", async () => {
    const hook = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    const historyUrl = hook.result.current.state.status === "ready" ? hook.result.current.state.prepared.urls.get(0) : undefined;
    expect(historyUrl).toBeDefined();

    hook.rerender({ target: { entryId: 7, owner: "favorites" } });
    await waitFor(() => {
      expect(hook.result.current.state.status).toBe("ready");
      expect(requestAt(1).owner).toBe("favorites");
    });
    expect(requestAt(1).generation).toBeGreaterThan(requestAt(0).generation);
    expect(vi.mocked(URL.revokeObjectURL)).toHaveBeenCalledWith(historyUrl);
    hook.unmount();
  });

  it("prepares nothing when the frame arrives after unmount", async () => {
    let resolveFrame: ((frame: DecodedDocumentFrame) => void) | null = null;
    mocks.getDocumentPreview.mockImplementation(() => new Promise<DecodedDocumentFrame>((resolve) => {
      resolveFrame = resolve;
    }));

    const hook = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1));
    const pending = buildFrame(requestAt(0));
    hook.unmount();

    await act(async () => {
      resolveFrame?.(pending);
    });
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
  });

  it("revokes prepared assets when the install-boundary check resolves after unmount", async () => {
    let resolveValidation: ((current: boolean) => void) | null = null;
    mocks.validateDocumentPreview.mockImplementation(() => new Promise<boolean>((resolve) => {
      resolveValidation = resolve;
    }));

    const hook = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(vi.mocked(URL.createObjectURL)).toHaveBeenCalledWith(expect.anything()));
    hook.unmount();

    await act(async () => {
      resolveValidation?.(true);
    });
    await waitFor(() => expect(vi.mocked(URL.revokeObjectURL)).toHaveBeenCalledWith("blob:prepared-0"));
  });

  it("re-validates at the install boundary and refuses a stale candidate", async () => {
    mocks.validateDocumentPreview.mockResolvedValue(false);
    const hook = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("error"));
    expect(mocks.validateDocumentPreview).toHaveBeenCalledTimes(1);
    expect(vi.mocked(URL.createObjectURL)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(URL.revokeObjectURL)).toHaveBeenCalledWith("blob:prepared-0");
    hook.unmount();
  });

  it("fails closed on a corrupt frame instead of falling back to another renderer", async () => {
    mocks.getDocumentPreview.mockRejectedValue(new DocumentParseError("invalid_magic", "bad frame"));
    const hook = renderHook(({ target }: { target: DocumentTarget }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget },
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("error"));
    expect(hook.result.current.state).toMatchObject({
      status: "error",
      failure: { kind: "corrupt", retryable: false },
    });
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    hook.unmount();
  });

  it("stays idle and requests nothing when there is no document target", async () => {
    const hook = renderHook(({ target }: { target: DocumentTarget | null }) => useDocumentPreview(target), {
      initialProps: { target: null },
    });
    expect(hook.result.current.state.status).toBe("idle");
    expect(mocks.getDocumentPreview).not.toHaveBeenCalled();
    hook.unmount();
  });

  it("retains current assets for unrelated history changes and replaces them after an archive changes the stamp", async () => {
    const hook = renderHook(() => useDocumentPreview(historyTarget));
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    await act(async () => hook.result.current.refresh());
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1);
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
    mocks.validateDocumentPreview.mockResolvedValueOnce(false);
    await act(async () => hook.result.current.refresh());
    await waitFor(() => expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:prepared-0");
    hook.unmount();
  });

  it("does not let a late refresh resurrect a closed preview or interrupt a restore", async () => {
    const hook = renderHook(({ target }: { target: DocumentTarget | null }) => useDocumentPreview(target), {
      initialProps: { target: historyTarget as DocumentTarget | null },
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    let finish!: (current: boolean) => void;
    mocks.validateDocumentPreview.mockImplementationOnce(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    let refresh!: Promise<void>;
    act(() => { refresh = hook.result.current.refresh(); });
    hook.rerender({ target: null });
    await act(async () => { finish(false); await refresh; });
    expect(hook.result.current.state.status).toBe("idle");
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1);
    hook.rerender({ target: historyTarget });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    await act(async () => hook.result.current.refresh(() => false));
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(2);
    hook.unmount();
  });

  it.each([false, true])("installs and cancels a preview on close with StrictMode=%s", async (strict) => {
    const hook = renderHook(() => useDocumentPreview(historyTarget), {
      wrapper: strict ? ({ children }) => createElement(StrictMode, null, children) : undefined,
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    const request = requestAt(mocks.getDocumentPreview.mock.calls.length - 1);
    hook.unmount();
    await waitFor(() => expect(mocks.cancelDocumentPreview).toHaveBeenCalledWith(request));
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:prepared-0");
  });

  it("never exposes the previous owner in the next committed render, even when the new request fails", async () => {
    const observations: string[] = [];
    const hook = renderHook(({ current }: { current: DocumentTarget }) => {
      const result = useDocumentPreview(current);
      useLayoutEffect(() => {
        if (result.state.status === "ready") observations.push(`${current.owner}:${result.state.target.owner}`);
      });
      return result;
    }, { initialProps: { current: historyTarget } });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    mocks.getDocumentPreview.mockRejectedValueOnce(new Error("new request fails"));
    hook.rerender({ current: { entryId: 7, owner: "favorites" } });
    await waitFor(() => expect(hook.result.current.state.status).toBe("error"));
    expect(observations).not.toContain("favorites:history");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:prepared-0");
    hook.unmount();
  });

  it("keeps the owned job occupied until a cancelled getter actually exits", async () => {
    let finish!: (value: DecodedDocumentFrame) => void;
    mocks.getDocumentPreview.mockImplementationOnce(() => new Promise<DecodedDocumentFrame>((resolve) => { finish = resolve; }));
    const hook = renderHook(({ current }: { current: DocumentTarget }) => useDocumentPreview(current), { initialProps: { current: historyTarget } });
    await waitFor(() => expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1));
    for (let entryId = 8; entryId < 25; entryId += 1) hook.rerender({ current: { entryId, owner: "history" } });
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1);
    await act(async () => finish(buildFrame(requestAt(0))));
    act(() => hook.result.current.retry());
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    hook.unmount();
  });
});
