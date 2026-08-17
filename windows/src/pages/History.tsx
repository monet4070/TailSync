import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import {
  cancelFileBatch,
  clearHistory,
  deleteEntry,
  getFileProgress,
  getHistoryCapabilities,
  getHistoryPage,
  getMigrationDiagnostics,
  getSettings,
  getSyncWarning,
  getVersion,
  closePreviewWindow,
  openPreviewWindow,
  syncPreviewWindowMinimized,
  restoreEntry,
  restoreFileBatch,
  setHistoryPinned,
  type FileProgress,
  type HistoryCapabilities,
  type HistoryCategory,
  type HistoryEntry,
  type MigrationDiagnostics,
} from "../tailsyncClient";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { useTransient } from "../hooks/useTransient";
import {
  useThumbnailCache,
  type ThumbnailData,
} from "../hooks/useThumbnailCache";
import { buildHistoryQuery } from "../utils/historyQuery";
import {
  GROUP_LABEL_KEYS,
  computeBatchInfos,
  groupEntriesByDate,
} from "../utils/historyGrouping";
import { useVisiblePolling } from "../hooks/useVisiblePolling";
import {
  DATE_FILTER_OPTIONS,
  dateBounds,
  dateInputValue,
  localCalendarContextKey,
  type DateFilter,
} from "../utils/historyFilters";
import { LatestRequest } from "../utils/asyncControl";
import {
  HISTORY_PAGE_SIZE,
  historyPageCount,
  normalizeHistoryPage,
} from "../utils/historyPagination";
import {
  ArrowLeft,
  ArrowLeftRight,
  ArrowRight,
  CalendarDays,
  Check,
  ChevronDown,
  ChevronUp,
  Clipboard,
  Code2,
  Database,
  File,
  Filter,
  Folder,
  Globe2,
  Image as ImageIcon,
  Pin,
  Search,
  SearchX,
  Square,
  Terminal,
  TriangleAlert,
  Trash2,
  Type,
  X,
} from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";

/* ── Types ──────────────────────────────────────────────────────── */

/* ── Constants ──────────────────────────────────────────────────── */

const PAGE_SIZE = HISTORY_PAGE_SIZE;
const VERSION_POLL_MS = 800;
const SETTINGS_POLL_MS = 5000;
const SEARCH_DEBOUNCE_MS = 250;
const MAX_CACHED_THUMBNAILS = PAGE_SIZE * 4;
const NEW_GLOW_DURATION_MS = 3000;
const RESTORE_FEEDBACK_DURATION_MS = 1500;
const COLLAPSED_BATCH_FILE_LIMIT = 2;
// Animate only the rows that can plausibly be visible in the history window.
// Animating an entire 50-row page creates dozens of WebView compositor layers.
const MAX_PAGE_ENTER_ITEMS = 12;
const HISTORY_CATEGORIES: HistoryCategory[] = [
  "text",
  "website",
  "code",
  "command",
  "structured_data",
  "path",
  "image",
  "file",
];
const CATEGORY_FILTERS: Array<"all" | HistoryCategory> = [
  "all",
  ...HISTORY_CATEGORIES,
];

const CATEGORY_ICONS = {
  text: Type,
  website: Globe2,
  code: Code2,
  command: Terminal,
  structured_data: Database,
  path: Folder,
  image: ImageIcon,
  file: File,
} satisfies Record<HistoryCategory, typeof Type>;

function resolvedCategory(entry: HistoryEntry): HistoryCategory {
  return entry.category && HISTORY_CATEGORIES.includes(entry.category)
    ? entry.category
    : entry.type;
}

function resolvedCategories(entry: HistoryEntry): HistoryCategory[] {
  const primary = resolvedCategory(entry);
  const categories = (entry.categories ?? []).filter((category) =>
    HISTORY_CATEGORIES.includes(category),
  );
  return [primary, ...categories.filter((category) => category !== primary)].filter(
    (category, index, values) => values.indexOf(category) === index,
  );
}

interface FilterOption {
  value: string;
  label: string;
  icon?: typeof Type;
  category?: HistoryCategory;
}

function FilterDropdown({
  value,
  options,
  label,
  testId,
  onChange,
}: {
  value: string;
  options: FilterOption[];
  label: string;
  testId: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value) ?? options[0];
  const SelectedIcon = selected?.icon;

  useEffect(() => {
    if (!open) return;
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div className="filter-dropdown" ref={rootRef} data-testid={testId}>
      <button
        type="button"
        className={`filter-trigger${open ? " is-open" : ""}${value !== "all" ? " is-filtered" : ""}`}
        aria-label={`${label}: ${selected?.label}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        {SelectedIcon && (
          <SelectedIcon
            className={
              selected.category
                ? `filter-category-icon ${selected.category}`
                : undefined
            }
            size={13}
            strokeWidth={1.8}
            aria-hidden="true"
          />
        )}
        <span>{selected?.label}</span>
        <ChevronDown size={13} strokeWidth={1.8} aria-hidden="true" />
      </button>
      {open && (
        <div className="filter-menu" role="listbox" aria-label={label}>
          {options.map((option) => {
            const OptionIcon = option.icon;
            const selectedOption = option.value === value;
            return (
              <button
                type="button"
                className={`filter-option${selectedOption ? " is-selected" : ""}`}
                role="option"
                aria-selected={selectedOption}
                key={option.value}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                {OptionIcon ? (
                  <OptionIcon
                    className={
                      option.category
                        ? `filter-category-icon ${option.category}`
                        : undefined
                    }
                    size={13}
                    strokeWidth={1.8}
                    aria-hidden="true"
                  />
                ) : (
                  <span className="filter-option-spacer" />
                )}
                <span>{option.label}</span>
                {selectedOption && <Check size={13} strokeWidth={2} aria-hidden="true" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}


/* ── Helpers ────────────────────────────────────────────────────── */

function formatTime(iso: string) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatSize(bytes: number) {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/**
 * Keyboard events from an editor belong to that editor, not the history
 * navigator.  In particular, the search field is auto-focused when this
 * window opens, so treating every Space key as a preview command would make
 * normal text entry impossible.
 */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT" ||
    target.tagName === "BUTTON" ||
    target.tagName === "A" ||
    Boolean(target.closest("[role='button'], [contenteditable]"))
  );
}

/**
 * A collapsed file batch represents the batch as a single logical item for
 * preview purposes.  Once the batch is expanded, each visible child keeps its
 * own identity and can be previewed independently.
 */
function resolvePreviewEntryId(
  focusedId: number,
  entries: HistoryEntry[],
  expandedBatches: Set<string>,
): number | null {
  const focusedEntry = entries.find((entry) => entry.id === focusedId);
  if (!focusedEntry) return null;
  const batchId = focusedEntry.batch_id;
  if (!batchId || expandedBatches.has(batchId)) return focusedEntry.id;

  const firstBatchEntry = entries
    .filter((entry) => entry.batch_id === batchId)
    .sort((left, right) => {
      const leftIndex = left.batch_index ?? Number.MAX_SAFE_INTEGER;
      const rightIndex = right.batch_index ?? Number.MAX_SAFE_INTEGER;
      return leftIndex - rightIndex || left.id - right.id;
    })[0];
  return firstBatchEntry?.id ?? focusedEntry.id;
}

/* ── Thumbnail canvas (renders raw RGBA data) ───────────────────── */

function ThumbnailCanvas({ data }: { data: ThumbnailData }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const expectedLength = data.width * data.height * 4;
    if (
      data.width <= 0 ||
      data.height <= 0 ||
      !Number.isSafeInteger(expectedLength)
    ) return;
    canvas.width = data.width;
    canvas.height = data.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let binaryStr: string;
    try {
      binaryStr = atob(data.b64);
    } catch {
      return;
    }
    if (binaryStr.length !== expectedLength) return;
    const bytes = new Uint8ClampedArray(expectedLength);
    for (let i = 0; i < expectedLength; i++) {
      bytes[i] = binaryStr.charCodeAt(i);
    }
    try {
      const imageData = new ImageData(bytes, data.width, data.height);
      ctx.putImageData(imageData, 0, 0);
    } catch {
      // Keep the placeholder when the WebView rejects malformed image data.
    }
  }, [data]);

  return <canvas ref={canvasRef} className="item-thumb" />;
}

function LazyThumbnail({
  id,
  data,
  onVisible,
  fallback,
}: {
  id: number;
  data?: ThumbnailData;
  onVisible: (id: number) => void;
  fallback: React.ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (data) return;
    const element = rootRef.current;
    if (!element || typeof IntersectionObserver === "undefined") {
      onVisible(id);
      return;
    }
    const observer = new IntersectionObserver(
      (records) => {
        if (records.some((record) => record.isIntersecting)) {
          onVisible(id);
          observer.disconnect();
        }
      },
      { root: element.closest(".history-list"), rootMargin: "96px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [data, id, onVisible]);

  return (
    <div className="item-preview" ref={rootRef}>
      {data ? <ThumbnailCanvas data={data} /> : fallback}
    </div>
  );
}

/* ── Component ──────────────────────────────────────────────────── */

export function History() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [totalEntries, setTotalEntries] = useState<number | null>(0);
  const [hasMoreEntries, setHasMoreEntries] = useState(false);
  const [capabilities, setCapabilities] = useState<HistoryCapabilities | null>(null);
  const [migrationDiagnostics, setMigrationDiagnostics] = useState<MigrationDiagnostics | null>(null);
  const { thumbnails, loadThumbnail, clear: clearThumbnails } =
    useThumbnailCache(MAX_CACHED_THUMBNAILS);
  const [keywordDraft, setKeywordDraft] = useState("");
  const [keyword, setKeyword] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<"all" | HistoryCategory>("all");
  const [selectedDateFilter, setSelectedDateFilter] = useState<DateFilter>("all");
  const [customStartDate, setCustomStartDate] = useState("");
  const [customEndDate, setCustomEndDate] = useState("");
  const [calendarNow, setCalendarNow] = useState(() => new Date());
  const calendarContextKey = useRef(localCalendarContextKey(calendarNow));
  const [page, setPage] = useState(0);
  const [pageAnimationRevision, setPageAnimationRevision] = useState(0);
  // `selectedId` is intentionally transient restore feedback.  Keep keyboard
  // focus independent so a row remains selected while the user navigates or
  // opens/closes its preview.
  const [focusedId, setFocusedId] = useState<number | null>(null);
  const [selectedId, flashSelectedId, clearSelectedId] = useTransient<number | null>(
    null,
    RESTORE_FEEDBACK_DURATION_MS,
  );
  const [loading, setLoading] = useState(false);
  const [newIds, setNewIds] = useState<Set<number>>(new Set());
  const [expandedBatches, setExpandedBatches] = useState<Set<string>>(new Set());
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [actionError, flashActionError] = useTransient("", RESTORE_FEEDBACK_DURATION_MS);
  const [syncWarning, flashSyncWarning] = useTransient("", 8000);
  const newGlowTimers = useRef<Set<number>>(new Set());

  const lastVersion = useRef<number>(0);
  const prevIds = useRef<Set<number>>(new Set());
  const lastQueryKey = useRef("");
  const historyRequests = useRef(new LatestRequest());
  const [fileProgress, setFileProgress] = useState<FileProgress | null>(null);
  const [progressBarEnabled, setProgressBarEnabled] = useState(true);
  useEffect(() => () => {
    newGlowTimers.current.forEach(window.clearTimeout);
    newGlowTimers.current.clear();
  }, []);
  useEffect(() => {
    const refreshCalendar = () => {
      const nextNow = new Date();
      const nextKey = localCalendarContextKey(nextNow);
      if (nextKey === calendarContextKey.current) return;
      calendarContextKey.current = nextKey;
      setCalendarNow(nextNow);
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshCalendar();
    };
    const intervalId = window.setInterval(refreshCalendar, 60_000);
    window.addEventListener("focus", refreshCalendar);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.clearInterval(intervalId);
      window.removeEventListener("focus", refreshCalendar);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, []);

  const { theme, resolvedColorTheme } = useTheme();
  const { t } = useI18n();
  const showActionError = useCallback(() => {
    flashActionError(t("history.actionFailed"));
  }, [t, flashActionError]);

  const categoryOptions = useMemo<FilterOption[]>(
    () =>
      CATEGORY_FILTERS.filter(
        (category) => category === "all" || !capabilities || capabilities.categories.includes(category),
      ).map((category) => ({
        value: category,
        label: t(`history.category.${category}`),
        icon: category === "all" ? Filter : CATEGORY_ICONS[category],
        category: category === "all" ? undefined : category,
      })),
    [capabilities, t],
  );
  const dateOptions = useMemo<FilterOption[]>(
    () =>
      DATE_FILTER_OPTIONS.map((filter) => ({
        value: filter,
        label: t(`history.date.${filter}`),
        icon: CalendarDays,
      })),
    [t],
  );
  const activeDateBounds = useMemo(
    () => dateBounds(selectedDateFilter, customStartDate, customEndDate, calendarNow),
    [selectedDateFilter, customStartDate, customEndDate, calendarNow],
  );
  const hasActiveFilters =
    Boolean(keywordDraft) || selectedCategory !== "all" || selectedDateFilter !== "all";
  const totalPages = historyPageCount(totalEntries ?? 0);
  const hasNext = totalEntries === null ? hasMoreEntries : page < totalPages - 1;
  const hasPrev = page > 0;

  const handleDateFilterChange = useCallback((value: string) => {
    const filter = value as DateFilter;
    setSelectedDateFilter(filter);
    if (filter === "custom" && !customStartDate && !customEndDate) {
      const today = dateInputValue(new Date());
      setCustomStartDate(today);
      setCustomEndDate(today);
    }
  }, [customStartDate, customEndDate]);

  /* ── Settings load ────────────────────────────────────────────── */

  const loadSettings = useCallback(async () => {
    try {
      const s = await getSettings();
      setProgressBarEnabled(s.progress_bar_enabled);
    } catch {}
  }, []);

  /* ── History loading ──────────────────────────────────────────── */

  const loadMigrationDiagnostics = useCallback(async () => {
    try {
      setMigrationDiagnostics(
        await getMigrationDiagnostics(),
      );
    } catch (error) {
      console.error("Failed to load migration diagnostics:", error);
    }
  }, []);

  const loadCapabilities = useCallback(async () => {
    try {
      setCapabilities(
        await getHistoryCapabilities(),
      );
    } catch {
      setCapabilities(null);
    }
  }, []);

  const loadHistory = useCallback(async (options?: { detectNewEntries?: boolean }) => {
    const detectNewEntries = options?.detectNewEntries ?? true;
    const requestGeneration = historyRequests.current.begin();
    if (!activeDateBounds.valid) {
      setEntries([]);
      setTotalEntries(0);
      setHasMoreEntries(false);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const query = buildHistoryQuery(
        keyword,
        selectedCategory,
        activeDateBounds,
        capabilities?.date_range_filter ?? true,
        PAGE_SIZE,
        page,
      );
      const queryKey = JSON.stringify(query);
      const result = await getHistoryPage(query);
      if (!historyRequests.current.isCurrent(requestGeneration)) return;

      if (result.total !== null) {
        const normalizedPage = normalizeHistoryPage(page, result.total);
        if (normalizedPage !== page) {
          setPage(normalizedPage);
          return;
        }
      }

      const queryChanged = lastQueryKey.current !== queryKey;
      setEntries(result.entries);
      setTotalEntries(result.total);
      setHasMoreEntries(result.has_more);
      if (queryChanged) {
        setPageAnimationRevision((revision) => revision + 1);
      }

      if (!queryChanged && detectNewEntries) {
        const incomingIds = new Set(result.entries.map((entry) => entry.id));
        const freshIds = new Set(
          [...incomingIds].filter((id) => !prevIds.current.has(id)),
        );
        if (freshIds.size > 0) {
          setNewIds((previous) => new Set([...previous, ...freshIds]));
          const timer = window.setTimeout(() => {
            newGlowTimers.current.delete(timer);
            setNewIds((previous) => {
              const next = new Set(previous);
              freshIds.forEach((id) => next.delete(id));
              return next;
            });
          }, NEW_GLOW_DURATION_MS);
          newGlowTimers.current.add(timer);
        }
      }
      lastQueryKey.current = queryKey;
      prevIds.current = new Set(result.entries.map((entry) => entry.id));

    } catch (e) {
      if (historyRequests.current.isCurrent(requestGeneration)) {
        console.error("Failed to load history:", e);
      }
    } finally {
      if (historyRequests.current.isCurrent(requestGeneration)) {
        setLoading(false);
      }
    }
  }, [
    activeDateBounds,
    capabilities,
    keyword,
    page,
    selectedCategory,
  ]);

  /* ── Polling ──────────────────────────────────────────────────── */

  useEffect(() => {
    void loadCapabilities();
    void loadMigrationDiagnostics();
  }, [loadCapabilities, loadMigrationDiagnostics]);

  useVisiblePolling(loadSettings, SETTINGS_POLL_MS);
  useVisiblePolling(async () => {
    try {
      const resp = await getVersion();
      if (resp.version !== lastVersion.current) {
        lastVersion.current = resp.version;
        await loadHistory();
      }
    } catch {
      /* ignore */
    }
    try {
      const warning = await getSyncWarning();
      if (warning?.kind === "expired_event") {
        flashSyncWarning(t("history.syncExpired").replace("{peer}", warning.peer));
      }
    } catch {
      /* ignore */
    }
    if (!progressBarEnabled) {
      setFileProgress(null);
      return;
    }
    try {
      const fp = await getFileProgress();
      setFileProgress(fp.active ? fp : null);
    } catch {
      /* ignore */
    }
  }, VERSION_POLL_MS);

  /* ── Search reset ─────────────────────────────────────────────── */

  useEffect(() => {
    const timer = window.setTimeout(
      () => setKeyword(keywordDraft),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [keywordDraft]);

  useEffect(() => {
    setPage(0);
  }, [keyword, selectedCategory, selectedDateFilter, customStartDate, customEndDate]);

  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

  // Keep a detached preview paired with this history window. Tauri does not
  // expose a dedicated minimized event on every platform, so resize and focus
  // changes both trigger a cheap state query. System close requests are
  // intercepted as well, otherwise Alt+F4 would leave the hidden preview page
  // holding decrypted data alive.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let disposed = false;
    const syncMinimized = () => {
      void appWindow.isMinimized()
        .then((minimized) => {
          if (!disposed) return syncPreviewWindowMinimized(minimized);
          return undefined;
        })
        .catch((error: unknown) => {
          if (!disposed) console.error("Could not sync preview-window minimization:", error);
        });
    };
    let unlistenResize: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    void appWindow.onResized(syncMinimized).then((stop) => {
      if (disposed) stop();
      else unlistenResize = stop;
    });
    void appWindow.onFocusChanged(syncMinimized).then((stop) => {
      if (disposed) stop();
      else unlistenFocus = stop;
    });
    void appWindow.onCloseRequested((event) => {
      event.preventDefault();
      void closePreviewWindow().finally(() => appWindow.hide());
    }).then((stop) => {
      if (disposed) stop();
      else unlistenClose = stop;
    });
    return () => {
      disposed = true;
      unlistenResize?.();
      unlistenFocus?.();
      unlistenClose?.();
    };
  }, []);

  // Filters, pagination, and deletions can replace the loaded page. Do not
  // leave keyboard selection pointing at an entry that is no longer visible.
  useEffect(() => {
    const loadedIds = new Set(entries.map((entry) => entry.id));
    if (focusedId !== null && !loadedIds.has(focusedId)) setFocusedId(null);
  }, [entries, focusedId]);

  /* ── Keyboard preview navigation ────────────────────────────── */

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Do not steal shortcuts from inputs/editors or from another handler.
      // Modifier chords and auto-repeat are deliberately ignored as well.
      if (
        event.defaultPrevented ||
        event.repeat ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey
      ) {
        return;
      }

      if (isEditableTarget(event.target)) return;
      if (event.code !== "Space") return;

      // Space is only handled when it has a selected history row.  This is
      // important while the search input is focused and when the list is
      // empty: the browser should retain its normal Space behaviour then.
      if (focusedId === null) return;

      const targetId = resolvePreviewEntryId(focusedId, entries, expandedBatches);
      if (targetId === null) return;
      const focusedEntry = entries.find((entry) => entry.id === focusedId);
      const batchId = focusedEntry?.batch_id ?? null;
      event.preventDefault();
      void openPreviewWindow(
        targetId,
        batchId !== null && !expandedBatches.has(batchId) ? batchId : null,
      ).catch((error: unknown) => {
        console.error("Could not open the preview window:", error);
        showActionError();
      });
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [entries, expandedBatches, focusedId, showActionError]);

  /* ── Actions ──────────────────────────────────────────────────── */

  const handleRestore = async (id: number) => {
    try {
      await restoreEntry(id);
      flashSelectedId(id);
    } catch (e) {
      console.error("Restore failed:", e);
      showActionError();
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteEntry(id);
      if (focusedId === id) setFocusedId(null);
      // A mutation refresh must preserve the current page animation context.
      // It must also avoid marking a row pulled up from the next page as new.
      await Promise.all([
        loadHistory({ detectNewEntries: false }),
        loadMigrationDiagnostics(),
      ]);
    } catch (e) {
      console.error("Delete failed:", e);
      showActionError();
    }
  };

  const handleClearHistory = async () => {
    setClearing(true);
    try {
      await clearHistory();
      setEntries([]);
      setTotalEntries(0);
      setHasMoreEntries(false);
      clearThumbnails();
      setPage(0);
      setFocusedId(null);
      clearSelectedId();
      setExpandedBatches(new Set());
      setShowClearConfirm(false);
      prevIds.current = new Set();
      lastQueryKey.current = "";
      await loadMigrationDiagnostics();
    } catch (e) {
      console.error("Clear history failed:", e);
      showActionError();
    } finally {
      setClearing(false);
    }
  };

  const handleRestoreBatch = async (batchId: string) => {
    try {
      await restoreFileBatch(batchId);
    } catch (error) {
      console.error("Batch restore failed:", error);
      showActionError();
    }
  };

  const handlePinnedChange = async (entry: HistoryEntry) => {
    const pinned = !entry.pinned;
    try {
      await setHistoryPinned(entry.id, pinned);
      setEntries((current) => current.map((item) =>
        item.id === entry.id ? { ...item, pinned } : item
      ));
    } catch (error) {
      console.error("Pin update failed:", error);
      showActionError();
    }
  };

  const handleCancelFileBatch = async (batchId: string) => {
    try {
      await cancelFileBatch(batchId);
    } catch (error) {
      console.error("Cancel file batch failed:", error);
      showActionError();
    }
  };

  /* ── Derived state ────────────────────────────────────────────── */

  const restoredEntry = selectedId
    ? entries.find((entry) => entry.id === selectedId)
    : null;

  /* ── Render ───────────────────────────────────────────────────── */

  return (
    <div
      className={`app ${theme} theme-${resolvedColorTheme}`}
      data-focused-entry-id={focusedId === null ? undefined : String(focusedId)}
    >
      {/* ── Title bar ── */}
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <ThemeLogo />
          <span className="titlebar-text">TailSync</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => {
            void closePreviewWindow().finally(() => getCurrentWindow().hide());
          }}
          title={t("history.close")}
          aria-label={t("history.close")}
        >
          <X size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>

      {/* ── Search ── */}
      <div className="history-toolbar">
        <div className="search-bar">
          <span className="search-icon">
            <Search size={18} strokeWidth={1.7} aria-hidden="true" />
          </span>
          <input
            type="text"
            placeholder={t("history.searchPlaceholder")}
            value={keywordDraft}
            onChange={(e) => setKeywordDraft(e.target.value)}
            autoFocus
          />
          <button
            className="clear-history-btn"
            type="button"
            disabled={totalEntries === 0}
            onClick={() => setShowClearConfirm(true)}
            title={t("history.clearAll")}
            aria-label={t("history.clearAll")}
          >
            <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
          </button>
        </div>

        <div className="history-filter-bar">
          <FilterDropdown
            value={selectedCategory}
            options={categoryOptions}
            label={t("history.categoryFilter")}
            testId="category-filter"
            onChange={(value) =>
              setSelectedCategory(value as "all" | HistoryCategory)
            }
          />
          {(capabilities?.date_range_filter ?? true) && (
            <FilterDropdown
              value={selectedDateFilter}
              options={dateOptions}
              label={t("history.dateFilter")}
              testId="date-filter"
              onChange={handleDateFilterChange}
            />
          )}
        </div>
      </div>

      {(capabilities?.date_range_filter ?? true) && selectedDateFilter === "custom" && (
        <div className="custom-date-range" data-testid="custom-date-range">
          <label>
            <span>{t("history.date.start")}</span>
            <input
              type="date"
              value={customStartDate}
              max={customEndDate || undefined}
              aria-invalid={!activeDateBounds.valid}
              aria-describedby={!activeDateBounds.valid ? "custom-date-error" : undefined}
              onChange={(event) => setCustomStartDate(event.target.value)}
            />
          </label>
          <span className="date-range-separator" aria-hidden="true">–</span>
          <label>
            <span>{t("history.date.end")}</span>
            <input
              type="date"
              value={customEndDate}
              min={customStartDate || undefined}
              aria-invalid={!activeDateBounds.valid}
              aria-describedby={!activeDateBounds.valid ? "custom-date-error" : undefined}
              onChange={(event) => setCustomEndDate(event.target.value)}
            />
          </label>
          {!activeDateBounds.valid && (
            <span className="date-range-error" id="custom-date-error" role="status">
              {t("history.date.invalid")}
            </span>
          )}
        </div>
      )}

      {hasActiveFilters && totalEntries !== null && totalEntries > 0 && (
        <div className="search-results-count">
          {totalEntries} {t(totalEntries === 1 ? "history.result" : "history.results")}
        </div>
      )}

      {migrationDiagnostics && migrationDiagnostics.unresolved_count > 0 && (
        <div className="migration-warning" role="status">
          <TriangleAlert size={15} strokeWidth={1.8} aria-hidden="true" />
          <span>
            {t("history.migrationWarningPrefix")} {migrationDiagnostics.unresolved_count}{" "}
            {t("history.migrationWarningSuffix")}
          </span>
        </div>
      )}

      {/* ── Main content area ── */}
      {loading && entries.length === 0 ? (
        /* Skeleton on initial load */
        <div className="skeleton-list">
          {[0, 1, 2, 4].map((i) => (
            <div className="skeleton-item" key={i}>
              <div className="skeleton-icon" />
              <div className="skeleton-lines">
                <div className="skeleton-line" />
                <div className="skeleton-line" />
                <div className="skeleton-line" />
              </div>
            </div>
          ))}
        </div>
      ) : entries.length === 0 ? (
        /* Empty state */
        <div className="empty-state">
          {hasActiveFilters ? (
            <>
              <div className="empty-state-illustration">
                <SearchX size={30} strokeWidth={1.35} aria-hidden="true" />
              </div>
              <div className="empty-state-title">
                {t("history.noMatches")}
              </div>
              <div className="empty-state-desc">
                {t("history.noMatchesDescription")}
              </div>
            </>
          ) : (
            <>
              <div className="empty-state-illustration">
                <Clipboard size={30} strokeWidth={1.35} aria-hidden="true" />
              </div>
              <div className="empty-state-title">
                {t("history.emptyTitle")}
              </div>
              <div className="empty-state-desc">
                {t("history.emptyDescription")}
              </div>
            </>
          )}
        </div>
      ) : (
        /* History list with date groups */
        <div className="history-list">
          <div className="history-page" key={pageAnimationRevision}>
          {(() => {
            const orderedGroups = groupEntriesByDate(entries, calendarNow);
            const batchInfos = computeBatchInfos(orderedGroups);
            let pageEnterIndex = 0;
            return orderedGroups.map(([group, groupEntries]) => (
              <div className="date-group" key={group}>
                <div className="date-header">
                  <span className="date-dot" />
                  {t(GROUP_LABEL_KEYS[group])}
                </div>
                {groupEntries.map((entry) => {
                  const { batchId, batchPosition, batchTotal, batchCount, isBatchStart } =
                    batchInfos.get(entry.id)!;
                  const batchExpanded = Boolean(batchId && expandedBatches.has(batchId));
                  if (
                    batchId
                    && batchCount > COLLAPSED_BATCH_FILE_LIMIT
                    && !batchExpanded
                    && batchPosition >= COLLAPSED_BATCH_FILE_LIMIT
                  ) {
                    return null;
                  }
                  const isNew = newIds.has(entry.id);
                  const isExpandedBatchReveal = Boolean(
                    batchId
                    && batchExpanded
                    && batchPosition >= COLLAPSED_BATCH_FILE_LIMIT,
                  );
                  const enterIndex = pageEnterIndex++;
                  const isPageEnterItem =
                    !isExpandedBatchReveal && enterIndex < MAX_PAGE_ENTER_ITEMS;
                  const categories = resolvedCategories(entry);
                  const category = categories[0];
                  const CategoryIcon = CATEGORY_ICONS[category];
                  return (
                      <div
                        className={entry.batch_id ? "history-batch-item" : undefined}
                        key={entry.id}
                      >
                      {isBatchStart && (
                        <div className="history-batch-header">
                          <span>
                            <Folder size={13} strokeWidth={1.8} aria-hidden="true" />{" "}
                            {entry.batch_status === "incomplete" && batchCount !== batchTotal
                              ? `${batchCount}/${batchTotal}`
                              : batchCount}{" "}
                            {t("history.files")}
                          </span>
                          <div className="history-batch-actions">
                            {entry.batch_status === "complete" ? (
                              <button
                                type="button"
                                onClick={() => void handleRestoreBatch(entry.batch_id!)}
                              >
                                <Clipboard size={12} strokeWidth={1.8} aria-hidden="true" />
                                {t("history.copyAll")}
                              </button>
                            ) : (
                              <span className="batch-incomplete">{t("history.incomplete")}</span>
                            )}
                            {batchId && batchCount > COLLAPSED_BATCH_FILE_LIMIT && (
                              <button
                                className="batch-toggle"
                                type="button"
                                aria-expanded={batchExpanded}
                                onClick={() => setExpandedBatches((current) => {
                                  const next = new Set(current);
                                  if (next.has(batchId)) next.delete(batchId);
                                  else next.add(batchId);
                                  return next;
                                })}
                              >
                                {batchExpanded ? (
                                  <ChevronUp size={12} strokeWidth={1.8} aria-hidden="true" />
                                ) : (
                                  <ChevronDown size={12} strokeWidth={1.8} aria-hidden="true" />
                                )}
                                {batchExpanded
                                  ? t("history.showLess")
                                  : `${t("history.showMore")} (${batchCount - COLLAPSED_BATCH_FILE_LIMIT})`}
                              </button>
                            )}
                          </div>
                        </div>
                      )}
                      <article
                        className={`history-item${isNew ? " is-new" : ""}${selectedId === entry.id ? " restored" : ""}${focusedId === entry.id ? " focused" : ""}${isExpandedBatchReveal ? " batch-expanded-item" : ""}${isPageEnterItem ? " page-enter-item" : ""}`}
                         style={{
                           animationDelay: isExpandedBatchReveal
                             ? `${Math.min(batchPosition - COLLAPSED_BATCH_FILE_LIMIT, 3) * 12}ms`
                             : isPageEnterItem
                               ? `${enterIndex * 20}ms`
                               : undefined,
                         }}
                         data-id={entry.id}
                         data-focused={focusedId === entry.id ? "true" : undefined}
                         tabIndex={0}
                         aria-selected={focusedId === entry.id}
                         onClick={(event) => {
                           if (
                             (event.target as HTMLElement).closest(
                               "button, a, [role='button']",
                             )
                           ) {
                             return;
                           }
                           setFocusedId(entry.id);
                           // Make a click establish real DOM focus as well as
                           // logical selection, even though the row is an
                           // otherwise non-interactive article.
                           event.currentTarget.focus({ preventScroll: true });
                         }}
                         onDoubleClick={() => handleRestore(entry.id)}
                         onContextMenu={(event) => {
                           event.preventDefault();
                           void handleDelete(entry.id);
                         }}
                       >
                        {entry.type === "image" ? (
                          <LazyThumbnail
                            id={entry.id}
                            data={thumbnails.get(entry.id)}
                            onVisible={loadThumbnail}
                            fallback={(
                              <div className={`item-icon ${category}`}>
                                <CategoryIcon size={15} strokeWidth={1.8} aria-hidden="true" />
                              </div>
                            )}
                          />
                        ) : (
                          <div className="item-preview">
                            <div className={`item-icon ${category}`}>
                              <CategoryIcon size={15} strokeWidth={1.8} aria-hidden="true" />
                            </div>
                          </div>
                        )}

                        <div className="item-content">
                          <div className="item-meta">
                            <span className="item-categories">
                              {categories.map((label) => (
                                <span className={`item-type ${label}`} key={label}>
                                  {t(`history.category.${label}`)}
                                </span>
                              ))}
                            </span>
                            <span className="item-time">
                              {formatTime(entry.timestamp)}
                            </span>
                            <span className="item-peer">
                              <ArrowLeftRight className="item-peer-icon" size={11} strokeWidth={1.8} aria-hidden="true" />
                              {entry.source_peer}
                            </span>
                          </div>
                          <div
                            className="item-desc"
                            title={entry.description}
                          >
                            {entry.description}
                          </div>
                          <div className="item-footer">
                            <span className="item-size">
                              {formatSize(entry.size_bytes)}
                            </span>
                            <button
                              className={`pin-entry${entry.pinned ? " active" : ""}`}
                              type="button"
                              title={entry.pinned ? t("history.unpin") : t("history.pin")}
                              onClick={(event) => {
                                event.stopPropagation();
                                void handlePinnedChange(entry);
                              }}
                            >
                              <Pin size={11} fill={entry.pinned ? "currentColor" : "none"} aria-hidden="true" />
                            </button>
                          </div>
                        </div>
                      </article>
                      </div>
                    );
                  })}
                </div>
            ));
          })()}
          </div>
        </div>
      )}

      {/* ── Pagination ── */}
      {entries.length > 0 && (
        <div className="pagination">
          <button
            className="page-btn"
            disabled={!hasPrev}
            onClick={() => {
              setPage((p) => p - 1);
              document
                .querySelector(".history-list")
                ?.scrollTo({ top: 0 });
            }}
          >
            <ArrowLeft size={14} strokeWidth={1.8} aria-hidden="true" />
            {t("history.prev")}
          </button>
          <span className="page-info">
            {totalEntries === null ? page + 1 : `${page + 1} / ${totalPages}`}
          </span>
          <button
            className="page-btn"
            disabled={!hasNext}
            onClick={() => {
              setPage((p) => p + 1);
              document
                .querySelector(".history-list")
                ?.scrollTo({ top: 0 });
            }}
          >
            {t("history.next")}
            <ArrowRight size={14} strokeWidth={1.8} aria-hidden="true" />
          </button>
        </div>
      )}

      {/* ── File transfer progress ── */}
      {progressBarEnabled && fileProgress?.active && (
        <div className="file-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${Math.round((fileProgress.sent / Math.max(fileProgress.total, 1)) * 100)}%`,
              }}
            />
          </div>
          <span className="progress-text">
            <strong>{Math.min(fileProgress.completed_files + 1, fileProgress.total_files)}/{fileProgress.total_files}</strong>
            <span title={fileProgress.name}>{fileProgress.name}</span>
            {fileProgress.device && <span>{fileProgress.device}</span>}
            <span>{formatSize(fileProgress.sent)} / {formatSize(fileProgress.total)}</span>
            <span>{formatSize(fileProgress.speed_bytes_per_second)}/s</span>
          </span>
          {fileProgress.can_stop && fileProgress.batch_id && (
            <button
              className="progress-stop"
              type="button"
              title={t("history.stopTransfer")}
              onClick={() => void handleCancelFileBatch(fileProgress.batch_id)}
            >
              <Square size={12} fill="currentColor" aria-hidden="true" />
              <span>{t("history.stopTransfer")}</span>
            </button>
          )}
        </div>
      )}

      {/* ── Toast ── */}
      {actionError ? (
        <div className="toast" role="alert">{actionError}</div>
      ) : syncWarning ? (
        <div className="toast sync-warning-toast" role="status">{syncWarning}</div>
      ) : restoredEntry && (
        <div className="toast" key={restoredEntry.id}>
          {t("history.restored")}
        </div>
      )}

      {showClearConfirm && (
        <div className="dialog-backdrop" onMouseDown={() => !clearing && setShowClearConfirm(false)}>
          <div
            className="confirm-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="clear-history-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="confirm-dialog-icon">
              <Trash2 size={22} strokeWidth={1.6} aria-hidden="true" />
            </div>
            <h2 id="clear-history-title">
              {t("history.clearConfirmTitle")}
            </h2>
            <p>
              {t("history.clearConfirmDescription")}
            </p>
            <div className="confirm-dialog-actions">
              <button type="button" onClick={() => setShowClearConfirm(false)} disabled={clearing}>
                {t("history.cancel")}
              </button>
              <button type="button" className="danger" onClick={handleClearHistory} disabled={clearing}>
                {t(clearing ? "history.clearing" : "history.clearAll")}
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
}
