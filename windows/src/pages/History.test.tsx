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
          return Promise.resolve({ entries: [entry], total: 1, has_more: false });
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

  it("keeps search and filters in one compact toolbar", async () => {
    render(<History />);

    await screen.findByText(entry.description);
    const toolbar = document.querySelector(".history-toolbar");
    expect(toolbar).not.toBeNull();
    expect(toolbar).toContainElement(screen.getByPlaceholderText("history.searchPlaceholder"));
    expect(toolbar).toContainElement(screen.getByTestId("category-filter"));
    expect(toolbar).toContainElement(screen.getByTestId("date-filter"));

    const categoryTrigger = screen.getByRole("button", {
      name: "history.categoryFilter: history.category.all",
    });
    expect(categoryTrigger).not.toHaveAttribute("title");
    fireEvent.click(categoryTrigger);
    expect(categoryTrigger).toHaveAttribute("aria-expanded", "true");
    expect(categoryTrigger).not.toHaveAttribute("title");
    fireEvent.click(screen.getByRole("option", { name: "history.category.image" }));
    expect(screen.getByRole("button", {
      name: "history.categoryFilter: history.category.image",
    })).toHaveClass("is-filtered");
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
        batch_count: 2,
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
        batch_count: 2,
        batch_status: "complete",
      },
    ];
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: batchEntries, total: batchEntries.length, has_more: false });
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

  it("collapses file batches to two rows and expands them on demand", async () => {
    const batchEntries = Array.from({ length: 6 }, (_, index) => ({
      ...entry,
      id: 20 + index,
      type: "file",
      description: `batch-file-${index + 1}`,
      category: "file",
      categories: ["file"],
      pinned: false,
      batch_id: "batch-large",
      batch_index: index,
      batch_total: 6,
      batch_count: 6,
      batch_status: "complete",
    }));
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: batchEntries, total: batchEntries.length, has_more: false });
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
    const showMore = await screen.findByRole("button", { name: "history.showMore (4)" });
    expect(screen.getByText("batch-file-2")).toBeInTheDocument();
    expect(screen.queryByText("batch-file-3")).toBeNull();

    fireEvent.click(showMore);
    expect(await screen.findByText("batch-file-6")).toBeInTheDocument();
    expect(screen.getByText("batch-file-3").closest(".history-item"))
      .toHaveClass("batch-expanded-item");
    expect(screen.getByText("batch-file-3").closest(".history-item"))
      .toHaveStyle({ animationDelay: "0ms" });
    expect(screen.getByText("batch-file-6").closest(".history-item"))
      .toHaveStyle({ animationDelay: "36ms" });

    fireEvent.click(screen.getByRole("button", { name: "history.showLess" }));
    expect(screen.queryByText("batch-file-3")).toBeNull();
  });

  it("shows the received and expected counts for an incomplete batch", async () => {
    const batchEntries = Array.from({ length: 3 }, (_, index) => ({
      ...entry,
      id: 40 + index,
      type: "file",
      description: `partial-file-${index + 1}`,
      category: "file",
      categories: ["file"],
      pinned: false,
      batch_id: "batch-partial",
      batch_index: index,
      batch_total: 4,
      batch_count: 3,
      batch_status: "incomplete",
    }));
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: batchEntries, total: batchEntries.length, has_more: false });
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
    expect(await screen.findByText("3/4 history.files")).toBeInTheDocument();
    expect(screen.getByText("history.incomplete")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "history.copyAll" })).toBeNull();
    expect(screen.getByRole("button", { name: "history.showMore (1)" })).toBeInTheDocument();
  });
});
