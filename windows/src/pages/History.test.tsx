import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { History } from "./History";

const { invokeMock, hideMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  hideMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: hideMock }),
}));
vi.mock("../hooks/useTheme", () => ({
  useTheme: () => ({ theme: "light", colorTheme: "tailsync" }),
}));
vi.mock("../hooks/useI18n", () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

const entry = {
  id: 7,
  timestamp: "2026-08-02T12:00:00Z",
  type: "text",
  description: "Regression entry",
  data_hash: "hash",
  size_bytes: 16,
  source_peer: "Mac",
  category: "text",
};

describe("History item actions", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: [entry], total: 1 });
        case "get_history_capabilities":
          return Promise.resolve({
            classifier_version: 1,
            categories: ["text", "image", "file"],
            multiple_labels: true,
            date_range_filter: true,
          });
        case "get_migration_diagnostics":
          return Promise.resolve({ unresolved_count: 0 });
        case "get_version":
          return Promise.resolve({ version: 0 });
        case "get_file_progress":
          return Promise.resolve({ active: false, name: "", sent: 0, total: 0 });
        default:
          return Promise.resolve(undefined);
      }
    });
  });

  it("keeps row actions gesture-only without rendering action buttons", async () => {
    render(<History />);

    await screen.findByText(entry.description);
    const item = document.querySelector<HTMLElement>(".history-item");
    expect(item).not.toBeNull();
    expect(document.querySelector(".item-actions")).toBeNull();
    expect(screen.queryByRole("button", { name: /history\.restoreEntry/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /history\.deleteEntry/ })).toBeNull();

    invokeMock.mockClear();
    fireEvent.doubleClick(item!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("restore_entry", { id: entry.id });
    });

    invokeMock.mockClear();
    fireEvent.contextMenu(item!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("delete_entry", { id: entry.id });
    });
  });

  it("offers copy-all for complete batches and persists pin changes", async () => {
    const batchEntries = [
      {
        ...entry,
        id: 11,
        type: "file",
        description: "report.pdf",
        category: "file",
        categories: ["file"],
        pinned: false,
        batch_id: "batch-1",
        batch_index: 0,
        batch_total: 2,
        batch_status: "complete",
      },
      {
        ...entry,
        id: 12,
        type: "file",
        description: "notes.txt",
        category: "file",
        categories: ["file"],
        pinned: false,
        batch_id: "batch-1",
        batch_index: 1,
        batch_total: 2,
        batch_status: "complete",
      },
    ];
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: batchEntries, total: batchEntries.length });
        case "get_history_capabilities":
          return Promise.resolve({
            classifier_version: 1,
            categories: ["text", "image", "file"],
            multiple_labels: true,
            date_range_filter: true,
          });
        case "get_migration_diagnostics":
          return Promise.resolve({ unresolved_count: 0 });
        case "get_version":
          return Promise.resolve({ version: 0 });
        case "get_file_progress":
          return Promise.resolve({ active: false });
        default:
          return Promise.resolve(undefined);
      }
    });

    render(<History />);
    const copyAll = await screen.findByRole("button", { name: "history.copyAll" });
    invokeMock.mockClear();
    fireEvent.click(copyAll);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("restore_file_batch", { batchId: "batch-1" });
    });

    invokeMock.mockClear();
    fireEvent.click(screen.getAllByTitle("history.pin")[0]);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_history_pinned", {
        id: 11,
        pinned: true,
      });
    });
  });
});
