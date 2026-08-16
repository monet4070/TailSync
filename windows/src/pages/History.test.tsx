import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { History } from "./History";

const { invokeMock, hideMock, stopListening, onCloseRequestedMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  hideMock: vi.fn(),
  stopListening: vi.fn(),
  onCloseRequestedMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    hide: hideMock,
    isMinimized: vi.fn(() => Promise.resolve(false)),
    onResized: vi.fn(() => Promise.resolve(stopListening)),
    onFocusChanged: vi.fn(() => Promise.resolve(stopListening)),
    onCloseRequested: onCloseRequestedMock,
  }),
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
    onCloseRequestedMock.mockImplementation(() => Promise.resolve(stopListening));
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

  it("closes the detached preview before handling a system history close", async () => {
    onCloseRequestedMock.mockImplementation(() => Promise.resolve(stopListening));
    render(<History />);
    await screen.findByText(entry.description);

    const handler = onCloseRequestedMock.mock.calls.at(-1)?.[0] as
      | ((event: { preventDefault: () => void }) => void)
      | undefined;
    expect(handler).toBeTypeOf("function");
    const preventDefault = vi.fn();
    handler?.({ preventDefault });

    expect(preventDefault).toHaveBeenCalledOnce();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("close_preview_window");
      expect(hideMock).toHaveBeenCalled();
    });
  });

  it("selects a row and opens the independent preview window with Space", async () => {
    render(<History />);

    const item = await screen.findByText(entry.description);
    const row = item.closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();

    // The search field starts focused, so Space there must remain ordinary
    // text-entry input and must not open a preview.
    const search = screen.getByPlaceholderText("history.searchPlaceholder");
    const searchSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    search.dispatchEvent(searchSpace);
    expect(searchSpace.defaultPrevented).toBe(false);
    expect(invokeMock).not.toHaveBeenCalledWith("open_preview_window", expect.anything());

    fireEvent.click(row!);
    expect(row).toHaveClass("focused");
    expect(row).toHaveAttribute("data-focused", "true");
    expect(document.querySelector(".app")).toHaveAttribute("data-focused-entry-id", String(entry.id));

    const openSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(openSpace);
    expect(openSpace.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("open_preview_window", {
        request: { entryId: entry.id, batchId: null },
      });
    });

    invokeMock.mockClear();
    const closeEscape = new KeyboardEvent("keydown", {
      key: "Escape",
      code: "Escape",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(closeEscape);
    expect(closeEscape.defaultPrevented).toBe(false);
    expect(invokeMock).not.toHaveBeenCalledWith("close_preview_window");

    const reopenSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(reopenSpace);
    expect(reopenSpace.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("open_preview_window", {
        request: { entryId: entry.id, batchId: null },
      });
    });

    // Keeping a row selected must not make Space steal focus back from the
    // search field if the user clicks it again.
    search.focus();
    const focusedSearchSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    search.dispatchEvent(focusedSearchSpace);
    expect(focusedSearchSpace.defaultPrevented).toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("ignores preview shortcuts with modifiers and key auto-repeat", async () => {
    render(<History />);
    const row = (await screen.findByText(entry.description)).closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    fireEvent.click(row!);

    for (const options of [
      { ctrlKey: true },
      { altKey: true },
      { metaKey: true },
      { shiftKey: true },
      { repeat: true },
    ]) {
      const event = new KeyboardEvent("keydown", {
        key: " ",
        code: "Space",
        bubbles: true,
        cancelable: true,
        ...options,
      });
      document.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(invokeMock).not.toHaveBeenCalledWith("open_preview_window", expect.anything());

    const pin = screen.getByTitle("history.pin");
    pin.focus();
    const buttonSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    pin.dispatchEvent(buttonSpace);
    expect(buttonSpace.defaultPrevented).toBe(false);
    expect(invokeMock).not.toHaveBeenCalledWith("open_preview_window", expect.anything());
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

  it("limits page-entry animation to the first visible rows", async () => {
    const pageEntries = Array.from({ length: 20 }, (_, index) => ({
      ...entry,
      id: 100 + index,
      description: `page-entry-${index + 1}`,
      data_hash: `hash-${index + 1}`,
    }));
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: pageEntries, total: pageEntries.length, has_more: false });
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
    await screen.findByText("page-entry-20");

    const rows = [...document.querySelectorAll<HTMLElement>(".history-item")];
    const animatedRows = rows.filter((row) => row.classList.contains("page-enter-item"));
    expect(rows).toHaveLength(20);
    expect(animatedRows).toHaveLength(12);
    expect(animatedRows[0]).toHaveStyle({ animationDelay: "0ms" });
    expect(animatedRows[11]).toHaveStyle({ animationDelay: "220ms" });
    expect(rows[12]).not.toHaveClass("page-enter-item");
  });

  it("keeps retained rows mounted when a context-menu delete refreshes the page", async () => {
    const pageEntries = Array.from({ length: 4 }, (_, index) => ({
      ...entry,
      id: 200 + index,
      description: `delete-page-${index + 1}`,
      data_hash: `delete-hash-${index + 1}`,
    }));
    let deleted = false;
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({
            entries: deleted ? pageEntries.slice(1) : pageEntries.slice(0, 3),
            total: deleted ? 3 : 4,
            has_more: false,
          });
        case "delete_entry":
          deleted = true;
          return Promise.resolve(undefined);
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
    const retainedRow = (await screen.findByText("delete-page-2"))
      .closest<HTMLElement>(".history-item");
    const deletedRow = screen.getByText("delete-page-1")
      .closest<HTMLElement>(".history-item");
    expect(retainedRow).not.toBeNull();
    expect(deletedRow).not.toBeNull();

    fireEvent.contextMenu(deletedRow!);

    await waitFor(() => {
      expect(screen.queryByText("delete-page-1")).toBeNull();
      expect(screen.getByText("delete-page-4")).toBeInTheDocument();
    });
    expect(screen.getByText("delete-page-2").closest(".history-item"))
      .toBe(retainedRow);
    expect(screen.getByText("delete-page-4").closest(".history-item"))
      .not.toHaveClass("is-new");
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

  it("previews the first item of a collapsed batch and each item after expansion", async () => {
    const batchEntries = Array.from({ length: 5 }, (_, index) => ({
      ...entry,
      id: 60 + index,
      type: "file",
      description: `preview-batch-${index + 1}`,
      category: "file",
      categories: ["file"],
      pinned: false,
      batch_id: "preview-batch",
      batch_index: index,
      batch_total: 5,
      batch_count: 5,
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
    const secondRow = (await screen.findByText("preview-batch-2")).closest<HTMLElement>(".history-item");
    expect(secondRow).not.toBeNull();
    fireEvent.click(secondRow!);
    const collapsedSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(collapsedSpace);
    expect(collapsedSpace.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("open_preview_window", {
        request: { entryId: 60, batchId: "preview-batch" },
      });
    });

    invokeMock.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "history.showMore (3)" }));
    const fourthRow = (await screen.findByText("preview-batch-4")).closest<HTMLElement>(".history-item");
    expect(fourthRow).not.toBeNull();
    fireEvent.click(fourthRow!);
    const expandedSpace = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(expandedSpace);
    expect(expandedSpace.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("open_preview_window", {
        request: { entryId: 63, batchId: null },
      });
    });
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
