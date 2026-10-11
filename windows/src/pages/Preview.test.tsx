import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Preview } from "./Preview";
import type { PreviewWindowSnapshot } from "../tailsyncClient";

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  restore: vi.fn<() => Promise<void>>(),
  pull: vi.fn<() => Promise<PreviewWindowSnapshot | null>>(),
  present: vi.fn(() => Promise.resolve()),
  closePreview: vi.fn(() => Promise.resolve()),
  closeHistory: vi.fn(() => Promise.resolve()),
  closeFavorites: vi.fn(() => Promise.resolve()),
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
  restoreEntry: mocks.restore, getPreviewWindowRequest: mocks.pull,
  presentPreviewWindow: mocks.present, closePreviewWindow: mocks.closePreview,
  closeHistoryWindow: mocks.closeHistory, closeFavoritesWindow: mocks.closeFavorites,
}));
vi.mock("../hooks/useTheme", () => ({ useTheme: () => ({ theme: "light" }) }));
vi.mock("../hooks/useI18n", () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock("../hooks/useRuntimeSnapshots", () => ({ useRuntimeSnapshots: vi.fn() }));
vi.mock("../preview/usePreviewWindowFrame", () => ({ usePreviewWindowFrame: () => true }));
vi.mock("../preview/usePreviewPayload", () => ({ usePreviewPayload: (target: { entryId: number } | null) => ({
  state: target ? { status: "ready", payload: {
    entry_id: target.entryId, kind: "text", name: `entry-${target.entryId}`, size_bytes: 4,
    data: new TextEncoder().encode("note"), width: null, height: null,
    batch: { batch_id: "batch", previous_entry_id: 6, next_entry_id: 8, item_index: 1, item_count: 3 },
  } } : { status: "idle" }, retry: vi.fn(),
}) }));
// Keep the real PreviewShell and Preview page; only the heavy format renderer is stubbed.
vi.mock("../preview/PreviewContent", () => ({ PreviewContent: () => <div>content</div> }));
const snapshot = (revision = 1, entryId = 7): PreviewWindowSnapshot => ({
  revision, entryId, batchId: "batch", owner: "history", takeFocus: false,
});
async function emit(name: string, payload: unknown) {
  await act(async () => mocks.listeners.get(name)?.({ payload }));
}
async function mount() {
  render(<Preview />);
  await screen.findByRole("heading", { name: "entry-7" });
  await waitFor(() => expect(mocks.present).toHaveBeenCalledWith(1));
}

describe("Preview keyboard restore integration", () => {
  beforeEach(() => {
    mocks.listeners.clear();
    vi.clearAllMocks();
    mocks.pull.mockResolvedValue(snapshot());
    mocks.restore.mockResolvedValue(undefined);
    mocks.closePreview.mockResolvedValue(undefined);
  });

  it.each(["Escape", "Enter"])("keeps preview content while %s waits for Windows to hide it", async (key) => {
    let hidden!: () => void;
    mocks.closePreview.mockImplementation(() => new Promise<void>(resolve => { hidden = resolve; }));
    await mount();
    fireEvent.keyDown(document.activeElement!, { key });
    await waitFor(() => expect(mocks.closePreview).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    expect(screen.queryByText("history.preview.loading")).toBeNull();
    expect(mocks.closeHistory).not.toHaveBeenCalled();
    await act(async () => hidden());
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    expect(screen.queryByText("history.preview.loading")).toBeNull();
  });

  it("a delayed close acknowledgement cannot clear a newly opened preview", async () => {
    let hidden!: () => void;
    mocks.closePreview.mockImplementation(() => new Promise<void>(resolve => { hidden = resolve; }));
    await mount();
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await emit("tailsync://preview-request", snapshot(3, 8));
    await act(async () => hidden());
    expect(screen.getByRole("heading", { name: "entry-8" })).toBeInTheDocument();
  });

  it("a native close notice keeps the last frame but retires restore and presentation", async () => {
    await mount();
    await emit("tailsync://preview-close", { revision: 2 });
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    expect(screen.queryByText("history.preview.loading")).toBeNull();
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));
    expect(mocks.restore).not.toHaveBeenCalled();
    expect(mocks.present).toHaveBeenCalledTimes(1);
    await emit("tailsync://preview-request", snapshot(3, 8));
    expect(screen.getByRole("heading", { name: "entry-8" })).toBeInTheDocument();
    expect(mocks.present).toHaveBeenLastCalledWith(3);
  });

  it("ignores repeat close input while hiding and allows another attempt if hiding fails", async () => {
    let reject!: (error: Error) => void;
    mocks.closePreview.mockImplementationOnce(() => new Promise<void>((_, fail) => { reject = fail; }));
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await mount();
      fireEvent.keyDown(document.activeElement!, { key: "Escape" });
      fireEvent.keyDown(document.activeElement!, { key: "Escape", repeat: true });
      fireEvent.keyDown(document.activeElement!, { key: "Escape" });
      fireEvent.keyDown(document.activeElement!, { key: "Enter" });
      expect(mocks.closePreview).toHaveBeenCalledTimes(1);
      expect(mocks.restore).not.toHaveBeenCalled();
      await act(async () => reject(new Error("native hide failed")));
      expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
      fireEvent.keyDown(document.activeElement!, { key: "Escape" });
      expect(mocks.closePreview).toHaveBeenCalledTimes(2);
    } finally {
      errorLog.mockRestore();
    }
  });

  it("Enter and restore button share one request; success closes preview then host without paste", async () => {
    let finish!: () => void;
    mocks.restore.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    await mount();
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));
    expect(mocks.restore).toHaveBeenCalledTimes(1);
    expect(mocks.closeHistory).not.toHaveBeenCalled();
    await act(async () => finish());
    expect(mocks.closePreview).toHaveBeenCalledWith("all");
    expect(mocks.closeHistory).toHaveBeenCalledTimes(1);
    expect(mocks.closePreview.mock.invocationCallOrder[0]).toBeLessThan(mocks.closeHistory.mock.invocationCallOrder[0]);
    expect(mocks.closeFavorites).not.toHaveBeenCalled();
  });

  it("Ctrl+Enter preserves target, consumes late snapshots, blocks navigation and reports success", async () => {
    let finish!: () => void;
    mocks.restore.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    await mount();
    fireEvent.keyDown(document.activeElement!, { key: "Enter", ctrlKey: true });
    await emit("tailsync://restore-state", { active: true, epoch: 1, previewRevision: 2 });
    await emit("tailsync://preview-request", snapshot(3, 9));
    fireEvent.click(screen.getByRole("button", { name: "history.preview.next" }));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight", altKey: true });
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    await emit("tailsync://restore-state", { active: false, epoch: 1 });
    await act(async () => finish());
    expect(await screen.findByRole("status")).toHaveTextContent("history.preview.restored");
    expect(mocks.closeHistory).not.toHaveBeenCalled();
    expect(mocks.closePreview).not.toHaveBeenCalled();
    await emit("tailsync://preview-request", snapshot(3, 9));
    expect(screen.queryByRole("heading", { name: "entry-9" })).not.toBeInTheDocument();
    await emit("tailsync://preview-request", snapshot(4, 8));
    expect(screen.getByRole("heading", { name: "entry-8" })).toBeInTheDocument();
  });

  it("host restore blocks preview restore/buttons and discards a late initial pull", async () => {
    let finishPull!: (snapshot: PreviewWindowSnapshot) => void;
    mocks.pull.mockImplementation(() => new Promise((resolve) => { finishPull = resolve; }));
    render(<Preview />);
    await emit("tailsync://preview-request", snapshot());
    await emit("tailsync://restore-state", { active: true, epoch: 2, previewRevision: 5 });
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));
    expect(mocks.restore).not.toHaveBeenCalled();
    await emit("tailsync://restore-state", { active: false, epoch: 2 });
    // A delayed active event from the same finished epoch cannot relock the UI.
    await emit("tailsync://restore-state", { active: true, epoch: 2 });
    await act(async () => finishPull(snapshot(9, 10)));
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement!, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(mocks.restore).toHaveBeenCalledTimes(1));
  });

  it("failure retains target and permits retry; Escape closes only preview", async () => {
    mocks.restore.mockRejectedValueOnce(new Error("clipboard busy"));
    await mount();
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    expect(await screen.findByRole("alert")).toHaveTextContent("history.preview.restoreError");
    expect(screen.getByRole("heading", { name: "entry-7" })).toBeInTheDocument();
    expect(mocks.closeHistory).not.toHaveBeenCalled();
    fireEvent.keyDown(document.activeElement!, { key: "Enter", ctrlKey: true });
    await screen.findByRole("status");
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(mocks.closePreview).toHaveBeenCalledTimes(1);
    expect(mocks.closeHistory).not.toHaveBeenCalled();
  });

  it("close revision tombstones late snapshots; IME and key repeat cannot restore", async () => {
    await mount();
    fireEvent.keyDown(document.activeElement!, { key: "Enter", isComposing: true });
    fireEvent.keyDown(document.activeElement!, { key: "Enter", keyCode: 229 });
    fireEvent.keyDown(document.activeElement!, { key: "Enter", repeat: true });
    expect(mocks.restore).not.toHaveBeenCalled();
    await emit("tailsync://preview-close", { revision: 4 });
    await emit("tailsync://preview-request", snapshot(3, 8));
    expect(screen.queryByRole("heading", { name: "entry-8" })).not.toBeInTheDocument();
    expect(mocks.present).toHaveBeenCalledTimes(1);
  });
});
