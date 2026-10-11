import { StrictMode, useLayoutEffect } from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildPngAsset, concatAssets, documentAsset, documentImageBlock, encodeDocumentFrame } from "../test/documentFrames";
import { decodeDocumentFrame, type DecodedDocumentFrame } from "../utils/documentPreview";
import type { DocumentRequest } from "../types/localContracts.generated";
import { prepareDocumentAssets } from "./documentAssets";
import { DocumentPreview } from "./renderers/DocumentPreview";
import { useDocumentPreview, type DocumentTarget } from "./useDocumentPreview";

const client = vi.hoisted(() => ({ getDocumentPreview: vi.fn(), validateDocumentPreview: vi.fn(), cancelDocumentPreview: vi.fn() }));
vi.mock("../tailsyncClient", () => client);

function frame(request: DocumentRequest): DecodedDocumentFrame {
  const asset = documentAsset(0, 0, buildPngAsset(4, 2, 25));
  return decodeDocumentFrame(encodeDocumentFrame(request, {
    version: 1, blocks: [documentImageBlock(0, 0)], assets: [asset.asset], partial: [], styles_partial: false, first_image: 0,
  }, concatAssets([asset])), request);
}

const target: DocumentTarget = { entryId: 7, owner: "history" };

describe("independent document lifecycle review", () => {
  beforeEach(() => {
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:review-${counter++}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    client.getDocumentPreview.mockImplementation(async (request: DocumentRequest) => frame(request));
    client.validateDocumentPreview.mockResolvedValue(true);
    client.cancelDocumentPreview.mockResolvedValue(undefined);
  });
  afterEach(() => vi.restoreAllMocks());

  it("cancels an installed backend request on close", async () => {
    const hook = renderHook(() => useDocumentPreview(target));
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    const request = client.getDocumentPreview.mock.calls[0][0];
    hook.unmount();
    await waitFor(() => expect(client.cancelDocumentPreview).toHaveBeenCalledWith(request));
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:review-0");
  });

  it("supports the real preview entrypoint StrictMode without requiring manual retry", async () => {
    const hook = renderHook(() => useDocumentPreview(target), {
      wrapper: ({ children }) => <StrictMode>{children}</StrictMode>,
    });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    hook.unmount();
  });

  it("never exposes the previous owner in the next committed render, even when the new request fails", async () => {
    const observations: string[] = [];
    const hook = renderHook(({ current }: { current: DocumentTarget }) => {
      const result = useDocumentPreview(current);
      useLayoutEffect(() => {
        if (result.state.status === "ready") observations.push(`${current.owner}:${result.state.target.owner}`);
      });
      return result;
    }, { initialProps: { current: target } });
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    client.getDocumentPreview.mockRejectedValueOnce(new Error("new request fails"));
    hook.rerender({ current: { entryId: 7, owner: "favorites" } });
    await waitFor(() => expect(hook.result.current.state.status).toBe("error"));
    expect(observations).not.toContain("favorites:history");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:review-0");
    hook.unmount();
  });

  it("keeps the owned job occupied until a cancelled getter actually exits", async () => {
    let finish!: (value: DecodedDocumentFrame) => void;
    client.getDocumentPreview.mockImplementationOnce(() => new Promise<DecodedDocumentFrame>((resolve) => { finish = resolve; }));
    const hook = renderHook(({ current }: { current: DocumentTarget }) => useDocumentPreview(current), { initialProps: { current: target } });
    await waitFor(() => expect(client.getDocumentPreview).toHaveBeenCalledTimes(1));
    for (let entryId = 8; entryId < 25; entryId += 1) hook.rerender({ current: { entryId, owner: "history" } });
    expect(client.getDocumentPreview).toHaveBeenCalledTimes(1);
    await act(async () => finish(frame(client.getDocumentPreview.mock.calls[0][0])));
    act(() => hook.result.current.retry());
    await waitFor(() => expect(hook.result.current.state.status).toBe("ready"));
    hook.unmount();
  });

  it("includes parsed-model and blob costs before admitting a tiny quota", async () => {
    const decoded = frame({ entry_id: 7, owner: "history", generation: 1, request_id: "quota" });
    await expect(prepareDocumentAssets(decoded, undefined, { maxRetainedBytes: decoded.assets.buffer.byteLength })).rejects.toMatchObject({ code: "too-large" });
    expect(URL.createObjectURL).not.toHaveBeenCalled();
  });

  it("rejects an already cancelled assetless frame", async () => {
    const request: DocumentRequest = { entry_id: 7, owner: "history", generation: 1, request_id: "empty" };
    const decoded = decodeDocumentFrame(encodeDocumentFrame(request, { version: 1, blocks: [], assets: [], partial: [], styles_partial: false }, new Uint8Array()), request);
    const controller = new AbortController();
    controller.abort();
    await expect(prepareDocumentAssets(decoded, controller.signal)).rejects.toMatchObject({ code: "cancelled" });
  });

  it("does not carry an image decode failure into the replacement document", async () => {
    const first = await prepareDocumentAssets(frame({ entry_id: 7, owner: "history", generation: 1, request_id: "first" }));
    const second = await prepareDocumentAssets(frame({ entry_id: 8, owner: "history", generation: 2, request_id: "second" }));
    const translate = (key: string) => key;
    const view = render(<DocumentPreview prepared={first} translate={translate} />);
    fireEvent.error(screen.getByTestId("preview-document-image"));
    expect(screen.getByTestId("preview-document-image-placeholder")).toBeInTheDocument();
    view.rerender(<DocumentPreview prepared={second} translate={translate} />);
    expect(screen.getByTestId("preview-document-image")).toHaveAttribute("src", "blob:review-1");
    view.unmount();
    first.release();
    second.release();
  });
});
