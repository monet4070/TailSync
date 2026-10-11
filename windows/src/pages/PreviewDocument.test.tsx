import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useLayoutEffect } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildPngAsset,
  concatAssets,
  documentAsset,
  documentImageBlock,
  documentParagraph,
  documentRun,
  encodeDocumentFrame,
} from "../test/documentFrames";
import { DocumentParseError, decodeDocumentFrame } from "../utils/documentPreview";
import type { PreviewWindowSnapshot } from "../tailsyncClient";
import { Preview } from "./Preview";

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  pull: vi.fn<() => Promise<PreviewWindowSnapshot | null>>(),
  present: vi.fn(() => Promise.resolve()),
  closePreview: vi.fn(() => Promise.resolve()),
  closeHistory: vi.fn(() => Promise.resolve()),
  closeFavorites: vi.fn(() => Promise.resolve()),
  restore: vi.fn(() => Promise.resolve()),
  getPreview: vi.fn(() => Promise.resolve(new ArrayBuffer(0))),
  getDocumentPreview: vi.fn(),
  validateDocumentPreview: vi.fn(() => Promise.resolve(true)),
  cancelDocumentPreview: vi.fn(() => Promise.resolve()),
  loadingPayloads: [] as string[],
  runtime: undefined as ((snapshot: { history_version: number }) => Promise<void>) | undefined,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, callback: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(name, callback);
    return Promise.resolve(() => mocks.listeners.delete(name));
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({
  onCloseRequested: () => Promise.resolve(() => {}),
}) }));
vi.mock("../tailsyncClient", () => ({
  restoreEntry: mocks.restore,
  getPreviewWindowRequest: mocks.pull,
  presentPreviewWindow: mocks.present,
  closePreviewWindow: mocks.closePreview,
  closeHistoryWindow: mocks.closeHistory,
  closeFavoritesWindow: mocks.closeFavorites,
  getPreview: mocks.getPreview,
  getDocumentPreview: mocks.getDocumentPreview,
  validateDocumentPreview: mocks.validateDocumentPreview,
  cancelDocumentPreview: mocks.cancelDocumentPreview,
}));
vi.mock("../hooks/useTheme", () => ({ useTheme: () => ({ theme: "light", themeAssetSlots: {} }) }));
vi.mock("../hooks/useI18n", () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock("../hooks/useRuntimeSnapshots", () => ({
  useRuntimeSnapshots: (callback: (snapshot: { history_version: number }) => Promise<void>) => { mocks.runtime = callback; },
}));
vi.mock("../preview/usePreviewWindowFrame", () => ({ usePreviewWindowFrame: () => true }));
vi.mock("../preview/PreviewShell", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../preview/PreviewShell")>();
  return {
    PreviewShell: (props: Parameters<typeof actual.PreviewShell>[0]) => {
      useLayoutEffect(() => {
        if (props.loading && props.payload) mocks.loadingPayloads.push(props.payload.name);
      });
      return <actual.PreviewShell {...props} />;
    },
  };
});

const snapshot = (documentEligible: boolean): PreviewWindowSnapshot => ({
  revision: 1,
  entryId: 7,
  batchId: null,
  owner: "history",
  takeFocus: false,
  documentEligible,
});

interface DocumentRequestLike {
  entry_id: number;
  owner: "history" | "favorites";
  generation: number;
  request_id: string;
}

function documentFrame(request: DocumentRequestLike) {
  const asset = documentAsset(0, 0, buildPngAsset(2, 2, 0x11));
  const document = {
    version: 1,
    blocks: [documentParagraph([documentRun("文稿正文😀")]), documentImageBlock(0, 0)],
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

describe("Preview document mode", () => {
  beforeEach(() => {
    mocks.listeners.clear();
    mocks.loadingPayloads.length = 0;
    vi.clearAllMocks();
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => "blob:document-asset");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    mocks.getDocumentPreview.mockImplementation(async (request: DocumentRequestLike) => documentFrame(request));
    mocks.validateDocumentPreview.mockResolvedValue(true);
  });

  it("refreshes only a stale saved document after a history notification without restoring or focusing", async () => {
    mocks.pull.mockResolvedValue(snapshot(true));
    render(<Preview />);
    await screen.findByTestId("preview-document");
    await act(async () => mocks.runtime?.({ history_version: 10 }));
    await act(async () => mocks.runtime?.({ history_version: 11 }));
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1);
    mocks.validateDocumentPreview.mockResolvedValueOnce(false);
    await act(async () => mocks.runtime?.({ history_version: 12 }));
    await waitFor(() => expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(2));
    await screen.findByTestId("preview-document");
    expect(mocks.present).toHaveBeenCalledTimes(1);
    expect(mocks.restore).not.toHaveBeenCalled();
    expect(mocks.closePreview).not.toHaveBeenCalled();
  });

  it("installs the controlled renderer for a document-eligible snapshot and never reads the raw preview", async () => {
    mocks.pull.mockResolvedValue(snapshot(true));
    render(<Preview />);

    await waitFor(() => expect(screen.getByTestId("preview-document")).toBeInTheDocument());
    expect(screen.getByTestId("preview-document").textContent).toContain("文稿正文😀");
    expect(screen.getAllByTestId("preview-document-image")).toHaveLength(1);
    expect(mocks.getPreview).not.toHaveBeenCalled();
    expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1);
    expect(mocks.getDocumentPreview.mock.calls[0][0]).toMatchObject({ entry_id: 7, owner: "history" });
  });

  it("shows the failure state for a corrupt document frame without falling back to the raw preview", async () => {
    mocks.pull.mockResolvedValue(snapshot(true));
    mocks.getDocumentPreview.mockRejectedValue(new DocumentParseError("unsupported_version", "bad frame"));
    render(<Preview />);

    await waitFor(() => expect(screen.getByTestId("preview-error")).toBeInTheDocument());
    expect(screen.queryByTestId("preview-document")).toBeNull();
    expect(mocks.getPreview).not.toHaveBeenCalled();
  });

  it("keeps the legacy raw preview path for entries that are not document-eligible", async () => {
    mocks.pull.mockResolvedValue(snapshot(false));
    render(<Preview />);

    await waitFor(() => expect(mocks.getPreview).toHaveBeenCalledWith(7, null));
    expect(mocks.getDocumentPreview).not.toHaveBeenCalled();
    expect(screen.queryByTestId("preview-document")).toBeNull();
  });

  it("retains explicit original restore after a document renderer failure without restoring automatically", async () => {
    mocks.pull.mockResolvedValue(snapshot(true));
    mocks.getDocumentPreview.mockRejectedValue(new DocumentParseError("unsupported_version", "bad frame"));
    render(<Preview />);
    await waitFor(() => expect(screen.getByTestId("preview-error")).toBeInTheDocument());
    expect(mocks.restore).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));
    await waitFor(() => expect(mocks.restore).toHaveBeenCalledWith(7));
  });

  it("keeps IME Return inert, restores once on Ctrl+Return, and closes with the favorites owner", async () => {
    mocks.pull.mockResolvedValue({ ...snapshot(true), owner: "favorites" });
    const view = render(<Preview />);
    await waitFor(() => expect(screen.getByTestId("preview-document")).toBeInTheDocument());
    const root = view.container.querySelector(".preview-app")!;
    fireEvent.compositionStart(root);
    fireEvent.keyDown(document, { key: "Enter", isComposing: true });
    fireEvent.compositionEnd(root);
    await act(async () => new Promise((resolve) => setTimeout(resolve, 100)));
    expect(mocks.restore).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(mocks.restore).toHaveBeenCalledTimes(1));
    expect(mocks.closePreview).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Enter" });
    await waitFor(() => expect(mocks.closeFavorites).toHaveBeenCalledTimes(1));
    expect(mocks.closePreview).toHaveBeenCalledWith("favorites");
    expect(mocks.closeHistory).not.toHaveBeenCalled();
  });

  it("does not pass a legacy payload or restore footer into a newly selected document", async () => {
    const body = new TextEncoder().encode("old text");
    const header = new TextEncoder().encode(JSON.stringify({ entry_id: 7, kind: "text", name: "legacy.txt", size_bytes: body.length, width: null, height: null }));
    const legacy = new Uint8Array(9 + header.length + body.length);
    legacy.set(new TextEncoder().encode("TSPV"));
    legacy[4] = 1;
    new DataView(legacy.buffer).setUint32(5, header.length, true);
    legacy.set(header, 9);
    legacy.set(body, 9 + header.length);
    mocks.getPreview.mockResolvedValueOnce(legacy.buffer);
    mocks.pull.mockResolvedValue(snapshot(false));
    const view = render(<Preview />);
    await screen.findByRole("heading", { name: "legacy.txt" });
    let finish!: (frame: ReturnType<typeof documentFrame>) => void;
    mocks.getDocumentPreview.mockImplementationOnce(() => new Promise<ReturnType<typeof documentFrame>>((resolve) => { finish = resolve; }));
    await act(async () => mocks.listeners.get("tailsync://preview-request")?.({ payload: { ...snapshot(true), revision: 2, entryId: 8 } }));
    await waitFor(() => expect(mocks.getDocumentPreview).toHaveBeenCalledTimes(1));
    await act(async () => finish(documentFrame(mocks.getDocumentPreview.mock.calls[0][0])));
    await screen.findByTestId("preview-document");
    view.unmount();
    expect(mocks.loadingPayloads).not.toContain("legacy.txt");
    expect(mocks.restore).not.toHaveBeenCalled();
  });
});
