import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { useVisiblePolling } from "../hooks/useVisiblePolling";
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

type HistoryCategory =
  | "text"
  | "website"
  | "code"
  | "command"
  | "structured_data"
  | "path"
  | "image"
  | "file";

interface HistoryEntry {
  id: number;
  timestamp: string;
  type: "text" | "image" | "file";
  description: string;
  data_hash: string;
  size_bytes: number;
  source_peer: string;
  category?: HistoryCategory;
  categories?: HistoryCategory[];
  category_confidence?: number;
  classifier_version?: number;
  pinned?: boolean;
  batch_id?: string | null;
  batch_index?: number | null;
  batch_total?: number | null;
  batch_count?: number | null;
  batch_status?: "complete" | "incomplete";
}

interface ThumbnailData {
  b64: string;
  width: number;
  height: number;
}

interface ImageThumbnail {
  id: number;
  thumbnail_b64: string;
  thumbnail_width: number;
  thumbnail_height: number;
}

interface HistoryPageResult {
  entries: HistoryEntry[];
  total: number | null;
  has_more: boolean;
}

interface HistoryCapabilities {
  classifier_version: number;
  categories: HistoryCategory[];
  multiple_labels: boolean;
  date_range_filter: boolean;
}

interface MigrationDiagnostics {
  unresolved_count: number;
}

interface SyncWarning {
  kind: "expired_event";
  peer: string;
  occurred_at_ms: number;
}

/* ── Constants ──────────────────────────────────────────────────── */

const PAGE_SIZE = HISTORY_PAGE_SIZE;
const VERSION_POLL_MS = 800;
const SETTINGS_POLL_MS = 5000;
const SEARCH_DEBOUNCE_MS = 250;
const MAX_CACHED_THUMBNAILS = PAGE_SIZE * 4;
const NEW_GLOW_DURATION_MS = 3000;
const RESTORE_FEEDBACK_DURATION_MS = 1500;
const COLLAPSED_BATCH_FILE_LIMIT = 2;
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

type DateFilter =
  | "all"
  | "today"
  | "yesterday"
  | "last7"
  | "last30"
  | "thisMonth"
  | "custom";

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
        title={`${label}: ${selected?.label}`}
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

function localDateFromInput(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > 31) {
    return null;
  }

  // setFullYear avoids JavaScript's special 1900 offset for years 0-99.
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(year, month - 1, day);
  if (
    Number.isNaN(date.getTime()) ||
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return null;
  }
  return date;
}

function dateInputValue(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function localCalendarContextKey(date: Date): string {
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
  return `${dateInputValue(date)}|${date.getTimezoneOffset()}|${timeZone}`;
}

function dateBounds(
  filter: DateFilter,
  customStart: string,
  customEnd: string,
  now: Date,
): { start: number | null; end: number | null; valid: boolean } {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);
  if (filter === "all") return { start: null, end: null, valid: true };
  if (filter === "today") return { start: today.getTime(), end: tomorrow.getTime(), valid: true };
  if (filter === "yesterday") {
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    return { start: yesterday.getTime(), end: today.getTime(), valid: true };
  }
  if (filter === "last7" || filter === "last30") {
    const start = new Date(today);
    start.setDate(start.getDate() - (filter === "last7" ? 6 : 29));
    return { start: start.getTime(), end: tomorrow.getTime(), valid: true };
  }
  if (filter === "thisMonth") {
    const start = new Date(today.getFullYear(), today.getMonth(), 1);
    const end = new Date(today.getFullYear(), today.getMonth() + 1, 1);
    return { start: start.getTime(), end: end.getTime(), valid: true };
  }
  const start = customStart ? localDateFromInput(customStart) : null;
  const inclusiveEnd = customEnd ? localDateFromInput(customEnd) : null;
  const end = inclusiveEnd ? new Date(inclusiveEnd) : null;
  if (end) end.setDate(end.getDate() + 1);
  const validInputs = (!customStart || start !== null) && (!customEnd || inclusiveEnd !== null);
  const ordered = !start || !inclusiveEnd || start.getTime() <= inclusiveEnd.getTime();
  return {
    start: start?.getTime() ?? null,
    end: end?.getTime() ?? null,
    valid: Boolean((customStart || customEnd) && validInputs && ordered),
  };
}

/* ── Date grouping ──────────────────────────────────────────────── */

type DateGroup = "today" | "yesterday" | "thisWeek" | "thisMonth" | "older";

function getDateGroup(dateStr: string, now: Date): DateGroup {
  const d = new Date(dateStr);
  if (Number.isNaN(d.getTime())) return "older";
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const itemDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());

  if (itemDate.getTime() === today.getTime()) return "today";
  if (itemDate.getTime() === yesterday.getTime()) return "yesterday";
  // Compare local calendar dates through UTC ordinals so DST transitions do
  // not turn a seven-day boundary into 6.96 or 7.04 elapsed days.
  const dayOrdinal = (date: Date) =>
    Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) /
    (1000 * 60 * 60 * 24);
  const diffDays = dayOrdinal(today) - dayOrdinal(itemDate);
  if (diffDays <= 7) return "thisWeek";
  if (diffDays <= 30) return "thisMonth";
  return "older";
}

const GROUP_ORDER: DateGroup[] = [
  "today",
  "yesterday",
  "thisWeek",
  "thisMonth",
  "older",
];

const GROUP_LABEL_KEYS: Record<DateGroup, string> = {
  today: "history.group.today",
  yesterday: "history.group.yesterday",
  thisWeek: "history.group.thisWeek",
  thisMonth: "history.group.thisMonth",
  older: "history.group.older",
};

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
  const [thumbnails, setThumbnails] = useState<Map<number, ThumbnailData>>(new Map());
  const thumbnailIds = useRef<Set<number>>(new Set());
  const [keywordDraft, setKeywordDraft] = useState("");
  const [keyword, setKeyword] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<"all" | HistoryCategory>("all");
  const [selectedDateFilter, setSelectedDateFilter] = useState<DateFilter>("all");
  const [customStartDate, setCustomStartDate] = useState("");
  const [customEndDate, setCustomEndDate] = useState("");
  const [calendarNow, setCalendarNow] = useState(() => new Date());
  const calendarContextKey = useRef(localCalendarContextKey(calendarNow));
  const [page, setPage] = useState(0);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [newIds, setNewIds] = useState<Set<number>>(new Set());
  const [expandedBatches, setExpandedBatches] = useState<Set<string>>(new Set());
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [actionError, setActionError] = useState("");
  const [syncWarning, setSyncWarning] = useState("");
  const restoreFeedbackTimer = useRef<number>(0);
  const actionErrorTimer = useRef<number>(0);
  const syncWarningTimer = useRef<number>(0);
  const newGlowTimers = useRef<Set<number>>(new Set());

  const lastVersion = useRef<number>(0);
  const prevIds = useRef<Set<number>>(new Set());
  const lastQueryKey = useRef("");
  const historyRequests = useRef(new LatestRequest());
  const [fileProgress, setFileProgress] = useState<{
    batch_id: string;
    name: string;
    sent: number;
    total: number;
    active: boolean;
    direction: "sending" | "receiving";
    device: string;
    completed_files: number;
    total_files: number;
    speed_bytes_per_second: number;
    status: string;
    can_stop: boolean;
  } | null>(null);
  const [progressBarEnabled, setProgressBarEnabled] = useState(true);
  useEffect(() => () => {
    window.clearTimeout(restoreFeedbackTimer.current);
    window.clearTimeout(actionErrorTimer.current);
    window.clearTimeout(syncWarningTimer.current);
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

  const { theme, colorTheme } = useTheme();
  const { t } = useI18n();

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
      (["all", "today", "yesterday", "last7", "last30", "thisMonth", "custom"] as DateFilter[]).map(
        (filter) => ({
          value: filter,
          label: t(`history.date.${filter}`),
          icon: CalendarDays,
        }),
      ),
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
      const s = await invoke<{
        progress_bar_enabled: boolean;
      }>("get_settings");
      setProgressBarEnabled(s.progress_bar_enabled);
    } catch {}
  }, []);

  /* ── History loading ──────────────────────────────────────────── */

  const loadThumbnail = useCallback(async (id: number) => {
    if (thumbnailIds.current.has(id)) return;
    thumbnailIds.current.add(id);
    try {
      const resp = await invoke<ImageThumbnail>("get_image_data", { id });
      if (resp.thumbnail_b64) {
        setThumbnails((current) => {
          const next = new Map(current);
          next.delete(id);
          next.set(id, {
            b64: resp.thumbnail_b64,
            width: resp.thumbnail_width,
            height: resp.thumbnail_height,
          });
          while (next.size > MAX_CACHED_THUMBNAILS) {
            const oldestId = next.keys().next().value;
            if (oldestId === undefined) break;
            next.delete(oldestId);
            thumbnailIds.current.delete(oldestId);
          }
          return next;
        });
      }
    } catch (e) {
      thumbnailIds.current.delete(id);
      console.error(`Thumbnail load failed for ${id}:`, e);
    }
  }, []);

  const loadMigrationDiagnostics = useCallback(async () => {
    try {
      setMigrationDiagnostics(
        await invoke<MigrationDiagnostics>("get_migration_diagnostics"),
      );
    } catch (error) {
      console.error("Failed to load migration diagnostics:", error);
    }
  }, []);

  const loadCapabilities = useCallback(async () => {
    try {
      setCapabilities(
        await invoke<HistoryCapabilities>("get_history_capabilities"),
      );
    } catch {
      setCapabilities(null);
    }
  }, []);

  const loadHistory = useCallback(async () => {
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
      const dateFilteringSupported = capabilities?.date_range_filter ?? true;
      const startTime = dateFilteringSupported && activeDateBounds.start !== null
        ? new Date(activeDateBounds.start).toISOString()
        : null;
      const endTime = dateFilteringSupported && activeDateBounds.end !== null
        ? new Date(activeDateBounds.end).toISOString()
        : null;
      const query = {
        keyword: keyword.trim() || null,
        category: selectedCategory === "all" ? null : selectedCategory,
        startTime,
        endTime,
        limit: PAGE_SIZE,
        offset: page * PAGE_SIZE,
      };
      const queryKey = JSON.stringify(query);
      const result = await invoke<HistoryPageResult>("get_history_page", query);
      if (!historyRequests.current.isCurrent(requestGeneration)) return;

      if (result.total !== null) {
        const normalizedPage = normalizeHistoryPage(page, result.total);
        if (normalizedPage !== page) {
          setPage(normalizedPage);
          return;
        }
      }

      setEntries(result.entries);
      setTotalEntries(result.total);
      setHasMoreEntries(result.has_more);

      const queryChanged = lastQueryKey.current !== queryKey;
      if (!queryChanged) {
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
      const resp = await invoke<{ version: number }>("get_version");
      if (resp.version !== lastVersion.current) {
        lastVersion.current = resp.version;
        await loadHistory();
      }
    } catch {
      /* ignore */
    }
    try {
      const warning = await invoke<SyncWarning | null>("get_sync_warning");
      if (warning?.kind === "expired_event") {
        setSyncWarning(t("history.syncExpired").replace("{peer}", warning.peer));
        window.clearTimeout(syncWarningTimer.current);
        syncWarningTimer.current = window.setTimeout(() => setSyncWarning(""), 8000);
      }
    } catch {
      /* ignore */
    }
    if (!progressBarEnabled) {
      setFileProgress(null);
      return;
    }
    try {
      const fp = await invoke<NonNullable<typeof fileProgress>>("get_file_progress");
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

  /* ── Actions ──────────────────────────────────────────────────── */

  const showActionError = useCallback(() => {
    setActionError(t("history.actionFailed"));
    window.clearTimeout(actionErrorTimer.current);
    actionErrorTimer.current = window.setTimeout(
      () => setActionError(""),
      RESTORE_FEEDBACK_DURATION_MS,
    );
  }, [t]);

  const handleRestore = async (id: number) => {
    try {
      await invoke("restore_entry", { id });
      setSelectedId(id);
      window.clearTimeout(restoreFeedbackTimer.current);
      restoreFeedbackTimer.current = window.setTimeout(
        () => setSelectedId(null),
        RESTORE_FEEDBACK_DURATION_MS,
      );
    } catch (e) {
      console.error("Restore failed:", e);
      showActionError();
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await invoke("delete_entry", { id });
      lastQueryKey.current = "";
      await Promise.all([loadHistory(), loadMigrationDiagnostics()]);
    } catch (e) {
      console.error("Delete failed:", e);
      showActionError();
    }
  };

  const handleClearHistory = async () => {
    setClearing(true);
    try {
      await invoke("clear_history");
      setEntries([]);
      setTotalEntries(0);
      setHasMoreEntries(false);
      setThumbnails(new Map());
      thumbnailIds.current.clear();
      setPage(0);
      setSelectedId(null);
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
      await invoke("restore_file_batch", { batchId });
    } catch (error) {
      console.error("Batch restore failed:", error);
      showActionError();
    }
  };

  const handlePinnedChange = async (entry: HistoryEntry) => {
    const pinned = !entry.pinned;
    try {
      await invoke("set_history_pinned", { id: entry.id, pinned });
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
      await invoke("cancel_file_batch", { batchId });
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
    <div className={`app ${theme} theme-${colorTheme}`}>
      {/* ── Title bar ── */}
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <ThemeLogo />
          <span className="titlebar-text">TailSync</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => getCurrentWindow().hide()}
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
          {(() => {
            // Group entries by date
            const groups: Record<string, HistoryEntry[]> = {};
            entries.forEach((entry) => {
              const g = getDateGroup(entry.timestamp, calendarNow);
              if (!groups[g]) groups[g] = [];
              groups[g].push(entry);
            });

            let itemIndex = 0;
            const batchPositions = new Map<string, number>();
            return GROUP_ORDER.map((group) => {
              const groupEntries = groups[group];
              if (!groupEntries) return null;

              return (
                <div className="date-group" key={group}>
                  <div className="date-header">
                    <span className="date-dot" />
                    {t(GROUP_LABEL_KEYS[group])}
                  </div>
                  {groupEntries.map((entry, groupIndex) => {
                    const batchId = entry.batch_id ?? null;
                    const batchPosition = batchId ? (batchPositions.get(batchId) ?? 0) : 0;
                    if (batchId) batchPositions.set(batchId, batchPosition + 1);
                    const batchTotal = entry.batch_total ?? 1;
                    const batchCount = entry.batch_count ?? batchTotal;
                    const batchExpanded = Boolean(batchId && expandedBatches.has(batchId));
                    if (
                      batchId
                      && batchCount > COLLAPSED_BATCH_FILE_LIMIT
                      && !batchExpanded
                      && batchPosition >= COLLAPSED_BATCH_FILE_LIMIT
                    ) {
                      return null;
                    }
                    const delay = itemIndex * 30;
                    itemIndex++;
                    const isNew = newIds.has(entry.id);
                    const isExpandedBatchReveal = Boolean(
                      batchId
                      && batchExpanded
                      && batchPosition >= COLLAPSED_BATCH_FILE_LIMIT,
                    );
                    const categories = resolvedCategories(entry);
                    const category = categories[0];
                    const CategoryIcon = CATEGORY_ICONS[category];
                    const isBatchStart = Boolean(
                      entry.batch_id && groupEntries[groupIndex - 1]?.batch_id !== entry.batch_id,
                    );
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
                        className={`history-item${isNew ? " is-new" : ""}${selectedId === entry.id ? " restored" : ""}${isExpandedBatchReveal ? " batch-expanded-item" : ""}`}
                         style={{
                           animationDelay: isExpandedBatchReveal
                             ? `${Math.min(batchPosition - COLLAPSED_BATCH_FILE_LIMIT, 3) * 12}ms`
                             : `${delay}ms`,
                         }}
                         data-id={entry.id}
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
              );
            });
          })()}
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
                ?.scrollTo({ top: 0, behavior: "smooth" });
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
                ?.scrollTo({ top: 0, behavior: "smooth" });
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
