import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import {
  CalendarDays,
  Check,
  ChevronDown,
  Code2,
  Database,
  File,
  Filter,
  Folder,
  Globe2,
  Image as ImageIcon,
  Terminal,
  Type,
} from "lucide-react";
import tailsyncIcon from "../../src-tauri/icons/32x32.png";

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

/* ── Constants ──────────────────────────────────────────────────── */

const PAGE_SIZE = 30;
const VERSION_POLL_MS = 800;
const NEW_GLOW_DURATION_MS = 3000;
const RESTORE_FEEDBACK_DURATION_MS = 1500;
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
        className={`filter-trigger${open ? " is-open" : ""}`}
        aria-label={label}
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
  if (bytes > 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes > 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/* ── Thumbnail canvas (renders raw RGBA data) ───────────────────── */

function ThumbnailCanvas({ data }: { data: ThumbnailData }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.width = data.width;
    canvas.height = data.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const binaryStr = atob(data.b64);
    const bytes = new Uint8ClampedArray(binaryStr.length);
    for (let i = 0; i < binaryStr.length; i++) {
      bytes[i] = binaryStr.charCodeAt(i);
    }
    const imageData = new ImageData(bytes, data.width, data.height);
    ctx.putImageData(imageData, 0, 0);
  }, [data]);

  return <canvas ref={canvasRef} className="item-thumb" />;
}

/* ── Component ──────────────────────────────────────────────────── */

export function History() {
  const [allEntries, setAllEntries] = useState<HistoryEntry[]>([]);
  const [thumbnails, setThumbnails] = useState<Map<number, ThumbnailData>>(new Map());
  const thumbnailIds = useRef<Set<number>>(new Set());
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
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [clearing, setClearing] = useState(false);
  const restoreFeedbackTimer = useRef<number>(0);

  const lastVersion = useRef<number>(0);
  const lastNotifiedId = useRef<number>(0);
  const prevIds = useRef<Set<number>>(new Set());
  const isInitialLoad = useRef(true);
  const [fileProgress, setFileProgress] = useState<{
    name: string;
    sent: number;
    total: number;
    active: boolean;
  } | null>(null);
  const [notificationsEnabled, setNotificationsEnabled] = useState(true);
  const [progressBarEnabled, setProgressBarEnabled] = useState(true);
  const notifEnabledRef = useRef(notificationsEnabled);
  const progressEnabledRef = useRef(progressBarEnabled);
  useEffect(() => {
    notifEnabledRef.current = notificationsEnabled;
  }, [notificationsEnabled]);
  useEffect(() => {
    progressEnabledRef.current = progressBarEnabled;
  }, [progressBarEnabled]);
  useEffect(
    () => () => window.clearTimeout(restoreFeedbackTimer.current),
    [],
  );
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

  const { theme } = useTheme();
  const { t } = useI18n();

  const categoryOptions = useMemo<FilterOption[]>(
    () =>
      CATEGORY_FILTERS.map((category) => ({
        value: category,
        label: t(`history.category.${category}`),
        icon: category === "all" ? Filter : CATEGORY_ICONS[category],
        category: category === "all" ? undefined : category,
      })),
    [t],
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
    Boolean(keyword) || selectedCategory !== "all" || selectedDateFilter !== "all";

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
        notifications_enabled: boolean;
        progress_bar_enabled: boolean;
      }>("get_settings");
      setNotificationsEnabled(s.notifications_enabled);
      setProgressBarEnabled(s.progress_bar_enabled);
    } catch {}
  }, []);

  /* ── Pagination ───────────────────────────────────────────────── */

  const filtered = useMemo(
    () => {
      const normalizedKeyword = keyword.toLowerCase();
      return allEntries.filter((entry) => {
        const categories = resolvedCategories(entry);
        const matchesCategory =
          selectedCategory === "all" || categories.includes(selectedCategory);
        const timestamp = new Date(entry.timestamp).getTime();
        const hasDateConstraint =
          activeDateBounds.start !== null || activeDateBounds.end !== null;
        const matchesDate =
          activeDateBounds.valid &&
          (!hasDateConstraint ||
            (!Number.isNaN(timestamp) &&
              (activeDateBounds.start === null || timestamp >= activeDateBounds.start) &&
              (activeDateBounds.end === null || timestamp < activeDateBounds.end)));
        const matchesKeyword =
          !normalizedKeyword ||
          entry.description.toLowerCase().includes(normalizedKeyword) ||
          entry.type.toLowerCase().includes(normalizedKeyword) ||
          entry.source_peer.toLowerCase().includes(normalizedKeyword) ||
          categories.some(
            (label) =>
              label.includes(normalizedKeyword) ||
              t(`history.category.${label}`).toLowerCase().includes(normalizedKeyword),
          );
        return matchesCategory && matchesDate && matchesKeyword;
      });
    },
    [activeDateBounds, allEntries, keyword, selectedCategory, t],
  );

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const entries = useMemo(
    () => filtered.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE),
    [filtered, page],
  );

  /* ── History loading ──────────────────────────────────────────── */

  const loadThumbnails = useCallback(async (ids: number[]) => {
    const loaded = new Map<number, ThumbnailData>();
    for (const id of ids) {
      if (thumbnailIds.current.has(id)) continue;
      thumbnailIds.current.add(id);
      try {
        const resp = await invoke<ImageThumbnail>("get_image_data", { id });
        if (resp.thumbnail_b64) {
          loaded.set(id, {
            b64: resp.thumbnail_b64,
            width: resp.thumbnail_width,
            height: resp.thumbnail_height,
          });
        }
      } catch (e) {
        thumbnailIds.current.delete(id);
        console.error(`Thumbnail load failed for ${id}:`, e);
      }
    }
    if (loaded.size > 0) {
      setThumbnails((current) => new Map([...current, ...loaded]));
    }
  }, []);

  const loadHistory = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<HistoryEntry[]>("get_history", {
        keyword: null,
        category: null,
        startTime: null,
        endTime: null,
        limit: 10000,
        offset: 0,
      });
      setAllEntries(result);

      // Detect newly added items (but not on initial load)
      if (!isInitialLoad.current) {
        const incomingIds = new Set(result.map((e) => e.id));
        const freshIds = new Set(
          [...incomingIds].filter((id) => !prevIds.current.has(id)),
        );
        if (freshIds.size > 0) {
          setNewIds((prev) => new Set([...prev, ...freshIds]));
          setTimeout(() => {
            setNewIds((prev) => {
              const next = new Set(prev);
              freshIds.forEach((id) => next.delete(id));
              return next;
            });
          }, NEW_GLOW_DURATION_MS);
        }
      }
      prevIds.current = new Set(result.map((e) => e.id));
      isInitialLoad.current = false;

      // Notification for latest entry
      if (result.length > 0) {
        const latest = result[0];
        if (
          latest.id > lastNotifiedId.current &&
          latest.source_peer !== "self"
        ) {
          lastNotifiedId.current = latest.id;
          showRemoteNotification(latest);
        }
      }

      // Load thumbnails for visible image entries
      const imageIds = result
        .filter((e) => e.type === "image")
        .map((e) => e.id);
      if (imageIds.length > 0) {
        loadThumbnails(imageIds);
      }
    } catch (e) {
      console.error("Failed to load history:", e);
    } finally {
      setLoading(false);
    }
  }, [loadThumbnails]);

  /* ── Polling ──────────────────────────────────────────────────── */

  useEffect(() => {
    loadSettings();
    let running = true;
    const poll = async () => {
      while (running) {
        loadSettings();
        try {
          const resp = await invoke<{ version: number }>("get_version");
          if (resp.version !== lastVersion.current) {
            lastVersion.current = resp.version;
            loadHistory();
          }
        } catch {
          /* ignore */
        }
        try {
          const fp = await invoke<{
            name: string;
            sent: number;
            total: number;
            active: boolean;
          }>("get_file_progress");
          setFileProgress(fp.active ? fp : null);
        } catch {
          /* ignore */
        }
        await new Promise((r) => setTimeout(r, VERSION_POLL_MS));
      }
    };
    poll();
    return () => {
      running = false;
    };
  }, [loadHistory, loadSettings]);

  /* ── Search reset ─────────────────────────────────────────────── */

  useEffect(() => {
    setPage(0);
  }, [keyword, selectedCategory, selectedDateFilter, customStartDate, customEndDate]);

  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

  /* ── Actions ──────────────────────────────────────────────────── */

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
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await invoke("delete_entry", { id });
      setAllEntries((prev) => prev.filter((e) => e.id !== id));
    } catch (e) {
      console.error("Delete failed:", e);
    }
  };

  const handleClearHistory = async () => {
    setClearing(true);
    try {
      await invoke("clear_history");
      setAllEntries([]);
      setThumbnails(new Map());
      thumbnailIds.current.clear();
      setPage(0);
      setSelectedId(null);
      setShowClearConfirm(false);
      prevIds.current = new Set();
    } catch (e) {
      console.error("Clear history failed:", e);
    } finally {
      setClearing(false);
    }
  };

  const showRemoteNotification = async (entry: HistoryEntry) => {
    if (!notifEnabledRef.current) return;
    let perm = await isPermissionGranted();
    if (!perm) {
      const req = await requestPermission();
      perm = req === "granted";
    }
    if (!perm) return;
    sendNotification({
      title: "TailSync",
      body:
        entry.type === "image"
          ? "📷 Image received"
          : entry.type === "file"
            ? `📎 ${entry.description}`
            : entry.description,
    });
  };

  /* ── Derived state ────────────────────────────────────────────── */

  const hasNext = page < totalPages - 1;
  const hasPrev = page > 0;
  const restoredEntry = selectedId
    ? allEntries.find((e) => e.id === selectedId)
    : null;

  /* ── Render ───────────────────────────────────────────────────── */

  return (
    <div className={`app ${theme}`}>
      {/* ── Title bar ── */}
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <div className="titlebar-logo">
            <img src={tailsyncIcon} alt="" />
          </div>
          <span className="titlebar-text">TailSync</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <button
          className="titlebar-close"
          onClick={() => getCurrentWindow().hide()}
          title={t("history.close")}
          aria-label={t("history.close")}
        >
          ✕
        </button>
      </div>

      {/* ── Search ── */}
      <div className="search-bar">
        <span className="search-icon">
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="M21 21l-4.35-4.35" />
          </svg>
        </span>
        <input
          type="text"
          placeholder={t("history.searchPlaceholder")}
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          autoFocus
        />
        <button
          className="clear-history-btn"
          type="button"
          disabled={allEntries.length === 0}
          onClick={() => setShowClearConfirm(true)}
          title={t("history.clearAll")}
          aria-label={t("history.clearAll")}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 10v6M14 10v6" />
          </svg>
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
        <FilterDropdown
          value={selectedDateFilter}
          options={dateOptions}
          label={t("history.dateFilter")}
          testId="date-filter"
          onChange={handleDateFilterChange}
        />
      </div>

      {selectedDateFilter === "custom" && (
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

      {hasActiveFilters && filtered.length > 0 && (
        <div className="search-results-count">
          {filtered.length} {t(filtered.length === 1 ? "history.result" : "history.results")}
        </div>
      )}

      {/* ── Main content area ── */}
      {loading && allEntries.length === 0 ? (
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
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <circle cx="11" cy="11" r="8" />
                  <path d="M21 21l-4.35-4.35" />
                </svg>
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
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" />
                  <path d="M9 12h6M12 9v6" />
                </svg>
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
            return GROUP_ORDER.map((group) => {
              const groupEntries = groups[group];
              if (!groupEntries) return null;

              return (
                <div className="date-group" key={group}>
                  <div className="date-header">
                    <span className="date-dot" />
                    {t(GROUP_LABEL_KEYS[group])}
                  </div>
                  {groupEntries.map((entry) => {
                    const delay = itemIndex * 30;
                    itemIndex++;
                    const isNew = newIds.has(entry.id);
                    const categories = resolvedCategories(entry);
                    const category = categories[0];
                    const CategoryIcon = CATEGORY_ICONS[category];
                    return (
                      <div
                        key={entry.id}
                        className={`history-item${isNew ? " is-new" : ""}${selectedId === entry.id ? " restored" : ""}`}
                        style={{ animationDelay: `${delay}ms` }}
                        data-id={entry.id}
                        onDoubleClick={() => handleRestore(entry.id)}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          handleDelete(entry.id);
                        }}
                      >
                        {entry.type === "image" &&
                        thumbnails.has(entry.id) ? (
                          <ThumbnailCanvas data={thumbnails.get(entry.id)!} />
                        ) : (
                          <div className={`item-icon ${category}`}>
                            <CategoryIcon size={15} strokeWidth={1.8} aria-hidden="true" />
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
                          </div>
                        </div>
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
      {allEntries.length > 0 && entries.length > 0 && (
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
            ← {t("history.prev")}
          </button>
          <span className="page-info">
            {page + 1} / {totalPages}
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
            {t("history.next")} →
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
                width: `${Math.round((fileProgress.sent / fileProgress.total) * 100)}%`,
              }}
            />
          </div>
          <span className="progress-text">
            {fileProgress.name} —{" "}
            {Math.round(fileProgress.sent / 1024)} /{" "}
            {Math.round(fileProgress.total / 1024)} KB
          </span>
        </div>
      )}

      {/* ── Toast ── */}
      {restoredEntry && (
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
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
              </svg>
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
