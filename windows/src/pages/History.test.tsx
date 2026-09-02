import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Favorites, History } from "./History";
import {
  LONG_PRESS_CHARGE_MS,
  LONG_PRESS_GRACE_MS,
} from "../hooks/useLongPressFavorite";

const {
  invokeMock,
  stopListening,
  onCloseRequestedMock,
  isAlwaysOnTopMock,
  setAlwaysOnTopMock,
} = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  stopListening: vi.fn(),
  onCloseRequestedMock: vi.fn(),
  isAlwaysOnTopMock: vi.fn(),
  setAlwaysOnTopMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMinimized: vi.fn(() => Promise.resolve(false)),
    isAlwaysOnTop: isAlwaysOnTopMock,
    setAlwaysOnTop: setAlwaysOnTopMock,
    onResized: vi.fn(() => Promise.resolve(stopListening)),
    onFocusChanged: vi.fn(() => Promise.resolve(stopListening)),
    onCloseRequested: onCloseRequestedMock,
  }),
}));
vi.mock("../hooks/useTheme", () => ({
  useTheme: () => ({ theme: "light", colorTheme: "tailsync", resolvedColorTheme: "tailsync" }),
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
let defaultEntryPinned = false;

function completeLongPress(row: HTMLElement, pointerId: number) {
  fireEvent.pointerDown(row, {
    button: 0,
    pointerId,
    clientX: 20,
    clientY: 20,
  });
  act(() => {
    vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS);
  });
  fireEvent.pointerUp(row, { pointerId });
}

describe("History item actions", () => {
  afterEach(() => vi.useRealTimers());

  beforeEach(() => {
    localStorage.clear();
    defaultEntryPinned = false;
    isAlwaysOnTopMock.mockResolvedValue(false);
    setAlwaysOnTopMock.mockResolvedValue(undefined);
    onCloseRequestedMock.mockImplementation(() => Promise.resolve(stopListening));
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({
            entries: [{ ...entry, pinned: defaultEntryPinned }],
            total: 1,
            has_more: false,
          });
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

  it("toggles the history window's always-on-top state and persists it", async () => {
    render(<History />);

    await screen.findByText(entry.description);
    const pin = screen.getByRole("button", { name: "history.pinWindow" });
    await waitFor(() => expect(pin).toBeEnabled());
    expect(pin).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(pin);
    await waitFor(() => {
      expect(setAlwaysOnTopMock).toHaveBeenCalledWith(true);
      expect(pin).toHaveAttribute("aria-pressed", "true");
    });
    expect(localStorage.getItem("tailsync-history-always-on-top")).toBe("true");

    fireEvent.click(pin);
    await waitFor(() => {
      expect(setAlwaysOnTopMock).toHaveBeenLastCalledWith(false);
      expect(pin).toHaveAttribute("aria-pressed", "false");
    });
    expect(localStorage.getItem("tailsync-history-always-on-top")).toBe("false");
  });

  it("shows a retryable notice when history loading fails", async () => {
    let attempts = 0;
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          attempts += 1;
          return attempts <= 2
            ? Promise.reject(new Error("database unavailable"))
            : Promise.resolve({
              entries: [{ ...entry, pinned: false }],
              total: 1,
              has_more: false,
            });
        case "get_history_capabilities":
          return Promise.resolve({
            classifier_version: 1,
            categories: ["text", "image", "file"],
            multiple_labels: true,
            date_range_filter: true,
          });
        case "get_migration_diagnostics":
          return Promise.resolve({ unresolved_count: 0 });
        default:
          return Promise.resolve(undefined);
      }
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(<History />);

    await waitFor(() => {
      expect(screen.getByText("history.loadError")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "history.retry" })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "history.retry" }));
    await screen.findByText(entry.description);
    expect(screen.queryByText("history.loadError")).toBeNull();
    expect(attempts).toBe(3);
    consoleError.mockRestore();
  });

  it("restores the persisted always-on-top state when the window mounts", async () => {
    localStorage.setItem("tailsync-history-always-on-top", "true");

    render(<History />);

    await screen.findByText(entry.description);
    await waitFor(() => {
      expect(setAlwaysOnTopMock).toHaveBeenCalledWith(true);
      expect(screen.getByRole("button", { name: "history.unpinWindow" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
  });

  it("keeps the window unpinned when restoring the persisted state fails", async () => {
    localStorage.setItem("tailsync-history-always-on-top", "true");
    setAlwaysOnTopMock.mockRejectedValueOnce(new Error("native failure"));

    render(<History />);

    await screen.findByText(entry.description);
    const pin = await screen.findByRole("button", { name: "history.pinWindow" });
    await waitFor(() => expect(pin).toBeEnabled());
    expect(pin).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps row actions gesture-only without rendering action buttons", async () => {
    render(<History />);

    await screen.findByText(entry.description);
    const item = document.querySelector<HTMLElement>(".history-item");
    expect(item).not.toBeNull();
    expect(document.querySelector(".item-actions")).toBeNull();
    expect(screen.queryByRole("button", { name: /history\.restoreEntry/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /history\.deleteEntry/ })).toBeNull();
    expect(item?.querySelector(".pin-entry")).toBeNull();
    expect(screen.queryByRole("button", { name: "history.pin" })).toBeNull();

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

  it("renders restore feedback in the footer instead of above the history list", async () => {
    render(<History />);

    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    fireEvent.doubleClick(row!);

    const notice = await screen.findByRole("status");
    const pagination = document.querySelector(".pagination");
    const historyList = document.querySelector(".history-list");
    expect(pagination).not.toBeNull();
    expect(historyList).not.toBeNull();
    expect(notice.previousElementSibling).toBe(pagination);
    expect(historyList!.compareDocumentPosition(notice) & Node.DOCUMENT_POSITION_FOLLOWING)
      .not.toBe(0);
  });

  it("uses one footer stamp for favorite state and long-press feedback", async () => {
    render(<History />);

    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    expect(screen.queryByRole("button", { name: "history.pin" })).toBeNull();
    expect(row?.querySelectorAll(".favorite-stamp")).toHaveLength(1);
    const footer = row!.querySelector<HTMLElement>(".item-footer");
    const stamp = row!.querySelector<HTMLElement>(".favorite-stamp");
    expect(footer).toContainElement(stamp);
    expect(row?.querySelector(".item-meta .favorite-stamp")).toBeNull();

    vi.useFakeTimers();
    completeLongPress(row!, 21);

    expect(row).toHaveClass("is-favorite");
    expect(row).toHaveClass("favorite-triggered");
    expect(row).toHaveClass("favorite-triggered-favorite");
    expect(row?.querySelectorAll(".favorite-stamp")).toHaveLength(1);
    expect(row?.querySelector(".pin-entry")).toBeNull();
    expect(screen.queryByRole("button", { name: "history.unpin" })).toBeNull();
  });

  it("keeps the footer stamp transient while an unfavorite fades out", async () => {
    defaultEntryPinned = true;
    render(<History />);

    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    expect(row).toHaveClass("is-favorite");

    vi.useFakeTimers();
    completeLongPress(row!, 22);

    expect(row).not.toHaveClass("is-favorite");
    expect(row).toHaveClass("favorite-triggered");
    expect(row).toHaveClass("favorite-triggered-unfavorite");
    act(() => vi.advanceTimersByTime(549));
    expect(row).toHaveClass("favorite-triggered");
    act(() => vi.advanceTimersByTime(1));
    expect(row).not.toHaveClass("favorite-triggered");
  });

  it("protects a favorite from the history context-menu deletion path", async () => {
    defaultEntryPinned = true;
    render(<History />);

    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    invokeMock.mockClear();

    fireEvent.contextMenu(row!);

    await waitFor(() => {
      expect(screen.getByText("history.favoriteProtected")).toBeInTheDocument();
    });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_entry", { id: entry.id });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_favorite_entry", { id: entry.id });
  });

  it("does not delete a logical item while its favorite mutation is in flight", async () => {
    const batchEntries = [
      {
        ...entry,
        id: 8,
        type: "file",
        description: "pending-a.txt",
        category: "file",
        pinned: false,
        batch_id: "pending-batch",
        batch_index: 0,
        batch_total: 2,
        batch_count: 2,
        batch_status: "complete",
      },
      {
        ...entry,
        id: 9,
        type: "file",
        description: "pending-b.txt",
        category: "file",
        pinned: false,
        batch_id: "pending-batch",
        batch_index: 1,
        batch_total: 2,
        batch_count: 2,
        batch_status: "complete",
      },
    ];
    let resolveFavorite!: (value: { affected_ids: number[]; favorite: boolean }) => void;
    const favoriteResponse = new Promise<{ affected_ids: number[]; favorite: boolean }>(
      (resolve) => { resolveFavorite = resolve; },
    );
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({ entries: batchEntries, total: 2, has_more: false });
        case "set_history_favorite":
          return favoriteResponse;
        case "get_history_capabilities":
          return Promise.resolve({
            classifier_version: 1,
            categories: ["text", "image", "file"],
            multiple_labels: true,
            date_range_filter: true,
          });
        case "get_migration_diagnostics":
          return Promise.resolve({ unresolved_count: 0 });
        default:
          return Promise.resolve(undefined);
      }
    });

    render(<History />);
    const sibling = (await screen.findByText("pending-b.txt"))
      .closest<HTMLElement>(".history-item");
    const firstRow = (await screen.findByText("pending-a.txt"))
      .closest<HTMLElement>(".history-item");
    vi.useFakeTimers();
    completeLongPress(firstRow!, 22);
    vi.useRealTimers();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_history_favorite", {
        id: 8,
        favorite: true,
      });
    });
    await waitFor(() => {
      expect(document.querySelectorAll(".history-item.is-favorite")).toHaveLength(2);
    });
    expect(sibling).toHaveClass("is-favorite");
    expect(document.querySelector(".pin-entry")).toBeNull();

    // A refresh started while the mutation is pending can still return the
    // pre-mutation database snapshot. It must not visually undo the completed
    // long-press fill while the write is in flight.
    const historyCallCountBeforeRefresh = invokeMock.mock.calls
      .filter(([command]) => command === "get_history_page").length;
    fireEvent.click(screen.getByRole("button", {
      name: "history.categoryFilter: history.category.all",
    }));
    fireEvent.click(screen.getByRole("option", { name: "history.category.file" }));
    await waitFor(() => {
      expect(invokeMock.mock.calls
        .filter(([command]) => command === "get_history_page").length)
        .toBeGreaterThan(historyCallCountBeforeRefresh);
    });
    expect(sibling).toHaveClass("is-favorite");

    fireEvent.contextMenu(sibling!);
    expect(invokeMock).not.toHaveBeenCalledWith("delete_entry", { id: 9 });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_favorite_entry", { id: 9 });

    resolveFavorite({ affected_ids: [8, 9], favorite: true });
    expect(document.querySelector(".pin-entry")).toBeNull();
  });

  it("reverts the optimistic favorite tint when the update fails", async () => {
    let rejectFavorite!: (reason?: unknown) => void;
    const favoriteResponse = new Promise<never>((_, reject) => {
      rejectFavorite = reject;
    });
    invokeMock.mockImplementation((command: string) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve({ progress_bar_enabled: true });
        case "get_history_page":
          return Promise.resolve({
            entries: [{ ...entry, pinned: false }],
            total: 1,
            has_more: false,
          });
        case "set_history_favorite":
          return favoriteResponse;
        case "get_history_capabilities":
          return Promise.resolve({
            classifier_version: 1,
            categories: ["text", "image", "file"],
            multiple_labels: true,
            date_range_filter: true,
          });
        case "get_migration_diagnostics":
          return Promise.resolve({ unresolved_count: 0 });
        default:
          return Promise.resolve(undefined);
      }
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(<History />);
    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();

    vi.useFakeTimers();
    completeLongPress(row!, 23);
    vi.useRealTimers();
    await waitFor(() => {
      expect(row).toHaveClass("is-favorite");
      expect(row).toHaveClass("favorite-triggered");
    });

    rejectFavorite(new Error("write failed"));
    await waitFor(() => {
      expect(row).not.toHaveClass("is-favorite");
      expect(row?.querySelector(".favorite-stamp")).not.toBeNull();
      expect(row?.querySelector(".pin-entry")).toBeNull();
      expect(screen.getByText("history.actionFailed")).toBeInTheDocument();
    });
    expect(consoleError).toHaveBeenCalledWith(
      "Favorite update failed:",
      expect.any(Error),
    );
    consoleError.mockRestore();
  });

  it("opens the separate favorites window from history", async () => {
    render(<History />);
    await screen.findByText(entry.description);

    invokeMock.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "favorites.open" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("open_favorites_window");
    });
  });

  it("queries and deletes entries through the favorites collection", async () => {
    defaultEntryPinned = true;
    render(<Favorites />);

    const row = (await screen.findByText(entry.description))
      .closest<HTMLElement>(".history-item");
    expect(row).not.toBeNull();
    expect(invokeMock).toHaveBeenCalledWith(
      "get_history_page",
      expect.objectContaining({ collection: "favorites" }),
    );

    invokeMock.mockClear();
    fireEvent.contextMenu(row!);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("delete_favorite_entry", { id: entry.id });
    });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_entry", { id: entry.id });
  });

  it("labels favorites distinctly without exposing the history pin control", async () => {
    render(<Favorites />);

    await screen.findByText(entry.description);
    expect(screen.getByText("favorites.title")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "history.pinWindow" })).toBeNull();
    expect(screen.queryByRole("button", { name: "history.unpinWindow" })).toBeNull();
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
      expect(invokeMock).toHaveBeenCalledWith("close_preview_window", { owner: "history" });
      expect(invokeMock).toHaveBeenCalledWith("close_history_window");
    });
  });

  it("closes only the favorites-owned preview on a favorites window close", async () => {
    onCloseRequestedMock.mockImplementation(() => Promise.resolve(stopListening));
    render(<Favorites />);
    await screen.findByText(entry.description);

    const handler = onCloseRequestedMock.mock.calls.at(-1)?.[0] as
      | ((event: { preventDefault: () => void }) => void)
      | undefined;
    const preventDefault = vi.fn();
    handler?.({ preventDefault });

    expect(preventDefault).toHaveBeenCalledOnce();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("close_preview_window", { owner: "favorites" });
      expect(invokeMock).toHaveBeenCalledWith("close_favorites_window");
    });
    expect(invokeMock).not.toHaveBeenCalledWith("close_history_window");
  });

  it("uses one blocking runtime snapshot instead of legacy high-frequency polls", async () => {
    render(<History />);
    await screen.findByText(entry.description);

    expect(invokeMock).toHaveBeenCalledWith("wait_runtime_snapshot", {
      sinceRevision: 0,
      waitMs: 2500,
      sinceNotificationId: 0,
    });
    expect(invokeMock).not.toHaveBeenCalledWith("get_version");
    expect(invokeMock).not.toHaveBeenCalledWith("get_file_progress");
    expect(invokeMock).not.toHaveBeenCalledWith("get_sync_warning");
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
        request: { entryId: entry.id, batchId: null, owner: "history" },
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
        request: { entryId: entry.id, batchId: null, owner: "history" },
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

    expect(screen.queryByTitle("history.pin")).toBeNull();
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

  it("offers copy-all for complete batches and updates every favorite batch row", async () => {
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
        case "set_history_favorite":
          return Promise.resolve({ affected_ids: [11, 12], favorite: true });
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
    const firstRow = (await screen.findByText("report.pdf"))
      .closest<HTMLElement>(".history-item");
    vi.useFakeTimers();
    completeLongPress(firstRow!, 24);
    vi.useRealTimers();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_history_favorite", {
        id: 11,
        favorite: true,
      });
    });
    await waitFor(() => {
      expect(document.querySelectorAll(".history-item.is-favorite")).toHaveLength(2);
    });
    expect(document.querySelector(".pin-entry")).toBeNull();
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
        request: { entryId: 60, batchId: "preview-batch", owner: "history" },
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
        request: { entryId: 63, batchId: null, owner: "history" },
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
