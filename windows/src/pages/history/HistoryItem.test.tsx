import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HistoryItem } from "./HistoryItem";

function fixture(nativeOnly = false, canRevokeSource = false) {
  const callbacks = {
    loadThumbnail: vi.fn(),
    setFocusedId: vi.fn(),
    handleRestore: vi.fn().mockResolvedValue(undefined),
    handleArchiveImages: vi.fn().mockResolvedValue(undefined),
    handleAuthorizeSource: vi.fn().mockResolvedValue(undefined),
    handleImportSourcePacket: vi.fn().mockResolvedValue(undefined),
    handleRevokeSource: vi.fn().mockResolvedValue(undefined),
    handleDelete: vi.fn().mockResolvedValue(undefined),
    handleFavoriteChange: vi.fn().mockResolvedValue(undefined),
    handleFavoriteProtected: vi.fn(),
  };
  const props = {
    ...callbacks,
    t: (key: string) => key,
    entry: {
      id: 41,
      timestamp: "2026-10-07T01:00:00Z",
      type: "composite" as const,
      description: "Parent readonly diagnostics fixture",
      data_hash: "parent-hash",
      size_bytes: 123,
      source_peer: "Synthetic",
      category: "text" as const,
      categories: ["text" as const],
      category_confidence: 100,
      classifier_version: 1,
      pinned: false,
      batch_id: null,
      batch_index: null,
      batch_total: null,
      batch_count: null,
      batch_status: "complete" as const,
      composite: {
        native_only: nativeOnly,
        native_item_count: nativeOnly ? 1 : 0,
        formats: ["text", "html"],
        restorable_formats: nativeOnly ? [] : ["text"],
        withheld_formats: [{ format: "png", code: "no_single_image" }],
        representation_count: 2,
        image_count: 0,
        completeness: "complete" as const,
      },
    },
    categories: ["text" as const],
    category: "text" as const,
    isNew: false,
    isSelected: false,
    isFocused: false,
    isExpandedBatchReveal: false,
    isPageEnterItem: false,
    enterIndex: 0,
    batchPosition: 0,
    thumbnails: new Map(),
    canRevokeSource,
    isFavoritesCollection: false,
  };
  return { callbacks, props };
}

function openRowActions() {
  const toggle = screen.getByRole("button", { name: "history.composite.moreActions" });
  if (toggle.getAttribute("aria-expanded") !== "true") fireEvent.click(toggle);
}

function openDetails() {
  fireEvent.click(screen.getByRole("button", { name: "history.composite.details" }));
  const value = document.querySelector<HTMLElement>(".item-diagnostics dd");
  expect(value).not.toBeNull();
  return value!;
}

describe("HistoryItem readonly diagnostics", () => {
  it("shows a document label and native-only thumbnail eligibility without portable image counts", () => {
    const { props, callbacks } = fixture(true);
    const entry = { ...props.entry, display: { version: 1, kind: "document" as const, thumbnail_eligible: true, cache_key: "th1:bound" } };
    render(<HistoryItem {...props} entry={entry} />);
    expect(screen.getByText("history.display.document")).toBeTruthy();
    expect(callbacks.loadThumbnail).toHaveBeenCalledWith(JSON.stringify(["all", "th1:bound", 48]), entry.id);
  });
  afterEach(() => vi.useRealTimers());

  it("dismisses keyboard-opened actions on Escape without forwarding it to the history window", () => {
    const { props } = fixture();
    render(<HistoryItem {...props} />);
    const historyKeyDown = vi.fn();
    document.addEventListener("keydown", historyKeyDown);
    try {
      openRowActions();
      const toggle = screen.getByRole("button", { name: "history.composite.moreActions" });
      fireEvent.keyDown(toggle, { key: "Escape" });
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      expect(screen.queryByRole("group", { name: "history.composite.moreActions" })).toBeNull();
      expect(historyKeyDown).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", historyKeyDown);
    }
  });

  it("does not restore the row on double-clicking readonly detail content", () => {
    const { callbacks, props } = fixture();
    render(<HistoryItem {...props} />);
    fireEvent.doubleClick(openDetails());
    expect(callbacks.handleRestore).not.toHaveBeenCalled();
  });

  it("does not delete the row on a context menu inside readonly details", () => {
    const { callbacks, props } = fixture();
    render(<HistoryItem {...props} />);
    fireEvent.contextMenu(openDetails());
    expect(callbacks.handleDelete).not.toHaveBeenCalled();
  });

  it("does not favorite the row while selecting or holding readonly detail text", () => {
    vi.useFakeTimers();
    const { callbacks, props } = fixture();
    render(<HistoryItem {...props} />);
    const value = openDetails();
    const pointerDown = new MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 20, clientY: 20 });
    Object.defineProperty(pointerDown, "pointerId", { value: 19 });
    fireEvent(value, pointerDown);
    act(() => vi.advanceTimersByTime(1000));
    const pointerUp = new MouseEvent("pointerup", { bubbles: true, button: 0 });
    Object.defineProperty(pointerUp, "pointerId", { value: 19 });
    fireEvent(value, pointerUp);
    expect(callbacks.handleFavoriteChange).not.toHaveBeenCalled();
  });

  it("keeps a live revoke action reachable after archival has become complete", () => {
    const { callbacks, props } = fixture(true, true);
    render(<HistoryItem {...props} />);
    openRowActions();
    fireEvent.click(screen.getByRole("button", { name: "history.composite.advanced" }));
    fireEvent.click(screen.getByRole("button", { name: "history.composite.revokeSource" }));
    expect(callbacks.handleRevokeSource).toHaveBeenCalledTimes(1);
    expect(callbacks.handleRestore).not.toHaveBeenCalled();
  });

  it("keeps the ordinary row whole restore while exposing the text backup entry", () => {
    const { callbacks, props } = fixture();
    const { container } = render(<HistoryItem {...props} />);
    openRowActions();
    expect(screen.getByRole("button", { name: "history.composite.copy.text" })).toBeTruthy();
    expect(container.querySelector(".item-diagnostics")).toBeNull();
    fireEvent.doubleClick(screen.getByRole("article"));
    expect(callbacks.handleRestore).toHaveBeenCalledExactlyOnceWith(props.entry.id);
  });

  it("expands the advanced submenu to reach the browser-source actions", () => {
    const { callbacks, props } = fixture();
    const entry = {
      ...props.entry,
      composite: { ...props.entry.composite, completeness: "external_references" as const },
    };
    render(<HistoryItem {...props} entry={entry} />);
    openRowActions();
    const toggle = screen.getByRole("button", { name: "history.composite.advanced" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "history.composite.authorizeSource" }));
    fireEvent.click(screen.getByRole("button", { name: "history.composite.importSourcePacket" }));
    expect(callbacks.handleAuthorizeSource).toHaveBeenCalledTimes(1);
    expect(callbacks.handleImportSourcePacket).toHaveBeenCalledTimes(1);
  });

  it("a live revoke does not reopen source authorization for a complete item", () => {
    const { props } = fixture(false, true);
    render(<HistoryItem {...props} />);
    openRowActions();
    fireEvent.click(screen.getByRole("button", { name: "history.composite.advanced" }));
    expect(screen.getByRole("button", { name: "history.composite.revokeSource" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "history.composite.authorizeSource" })).toBeNull();
    expect(screen.queryByRole("button", { name: "history.composite.importSourcePacket" })).toBeNull();
  });

  it("keeps the advanced submenu out of a plain-text row", () => {
    const { props } = fixture();
    const entry = {
      ...props.entry,
      composite: {
        ...props.entry.composite,
        formats: ["text"],
        restorable_formats: ["text"],
        image_count: 0,
        representation_count: 1,
        withheld_formats: [],
        completeness: "complete" as const,
      },
    };
    render(<HistoryItem {...props} entry={entry} />);
    expect(screen.queryByRole("button", { name: "history.composite.copy.text" })).toBeNull();
    expect(screen.queryByRole("button", { name: "history.composite.advanced" })).toBeNull();
  });

  it("labels the network-image completion with its own hint", () => {
    const { props } = fixture();
    const entry = {
      ...props.entry,
      composite: { ...props.entry.composite, completeness: "external_references" as const },
    };
    render(<HistoryItem {...props} entry={entry} />);
    openRowActions();
    const archive = screen.getByRole("button", { name: "history.composite.archiveImages" });
    expect(archive.getAttribute("title")).toBe("history.composite.archiveImagesHint");
  });
  it("keeps the detail toggle context menu readonly", () => {
    const { callbacks, props } = fixture();
    render(<HistoryItem {...props} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "history.composite.details" }));
    expect(callbacks.handleDelete).not.toHaveBeenCalled();
  });

  it("owns detail keyboard events and returns focus on Escape", () => {
    const { callbacks, props } = fixture();
    render(<HistoryItem {...props} />);
    const toggle = screen.getByRole("button", { name: "history.composite.details" });
    const value = openDetails();
    const panel = value.closest<HTMLElement>(".history-row-popover")!;
    const observer = vi.fn();
    document.addEventListener("keydown", observer);
    try {
      panel.focus();
      fireEvent.keyDown(value, { key: "Enter" });
      fireEvent.keyDown(value, { key: " ", code: "Space" });
      expect(observer).not.toHaveBeenCalled();
      expect(callbacks.handleRestore).not.toHaveBeenCalled();
      expect(toggle.getAttribute("aria-controls")).toBe(panel.id);
      fireEvent.keyDown(value, { key: "Escape" });
      expect(document.querySelector(".item-diagnostics")).toBeNull();
      expect(document.activeElement).toBe(toggle);
      expect(observer).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", observer);
    }
  });

  it("uses separate accessible detail owners for multiple rows", () => {
    const { props } = fixture();
    const second = { ...props, entry: { ...props.entry, id: 42, description: "Second fixture" } };
    render(<><HistoryItem {...props} /><HistoryItem {...second} /></>);
    const toggles = screen.getAllByRole("button", { name: "history.composite.details" });
    expect(toggles[0].id).not.toBe(toggles[1].id);
    fireEvent.click(toggles[0]);
    expect(toggles[0].getAttribute("aria-expanded")).toBe("true");
    expect(toggles[1].getAttribute("aria-expanded")).toBe("false");
    const panel = document.getElementById(toggles[0].getAttribute("aria-controls")!);
    expect(panel?.getAttribute("aria-labelledby")).toBe(toggles[0].id);
  });

  it("renders details outside the row and dismisses when the list scrolls", () => {
    const { props } = fixture();
    const { container } = render(<div className="app"><div className="history-list"><HistoryItem {...props} /></div></div>);
    openDetails();
    const row = screen.getByRole("article");
    const panel = document.querySelector(".history-row-popover")!;
    expect(row.contains(panel)).toBe(false);
    expect(container.querySelector(".app")?.contains(panel)).toBe(true);
    fireEvent.scroll(container.querySelector(".history-list")!);
    expect(document.querySelector(".history-row-popover")).toBeNull();
  });

  it("keeps scrolling inside details open and dismisses on an outside pointer press", () => {
    const { props, callbacks } = fixture();
    render(<HistoryItem {...props} />);
    const value = openDetails();
    const panel = value.closest<HTMLElement>(".history-row-popover")!;
    fireEvent.scroll(panel);
    expect(document.querySelector(".history-row-popover")).toBe(panel);
    fireEvent.pointerDown(document.body);
    expect(document.querySelector(".history-row-popover")).toBeNull();
    expect(callbacks.handleDelete).not.toHaveBeenCalled();
    expect(callbacks.handleRestore).not.toHaveBeenCalled();
  });

  it("keeps only one row popover open across keyboard activation of different rows", () => {
    const { props } = fixture();
    render(<><HistoryItem {...props} /><HistoryItem {...props} entry={{ ...props.entry, id: 42 }} /></>);
    const toggles = screen.getAllByRole("button", { name: "history.composite.details" });
    fireEvent.click(toggles[0]);
    fireEvent.click(toggles[1]);
    expect(document.querySelectorAll(".history-row-popover")).toHaveLength(1);
    expect(toggles[0].getAttribute("aria-expanded")).toBe("false");
    expect(toggles[1].getAttribute("aria-expanded")).toBe("true");
  });

  it("navigates action buttons without restoring the row and returns focus on Escape", () => {
    const { props, callbacks } = fixture(false, true);
    render(<HistoryItem {...props} />);
    openRowActions();
    const panel = screen.getByRole("group", { name: "history.composite.moreActions" });
    fireEvent.keyDown(panel, { key: "ArrowUp" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "history.composite.advanced" }));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "history.composite.copy.text" }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "history.composite.moreActions" }));
    expect(callbacks.handleRestore).not.toHaveBeenCalled();
  });

});
