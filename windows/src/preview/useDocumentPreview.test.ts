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
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:prepared-${counter++}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    mocks.getDocumentPreview.mockImplementation(async (request: DocumentRequestLike) => buildFrame(request));
    mocks.validateDocumentPreview.mockResolvedValue(true);
  });

  afterEach(() => vi.restoreAllMocks());

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
});
