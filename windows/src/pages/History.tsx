import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import {
  cancelFileBatch,
  clearHistory,
  deleteEntry,
  deleteFavoriteEntry,
  getHistoryCapabilities,
  getHistoryPage,
  getMigrationDiagnostics,
  getSettings,
  closeHistoryWindow,
  closeFavoritesWindow,
  closePreviewWindow,
  openFavoritesWindow,
  openPreviewWindow,
  syncPreviewWindowMinimized,
  restoreEntry,
  restoreFileBatch,
  setHistoryFavorite,
  type FileProgress,
  type HistoryCapabilities,
  type HistoryCategory,
  type HistoryCollection,
  type HistoryEntry,
  type MigrationDiagnostics,
} from "../tailsyncClient";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { useTransient } from "../hooks/useTransient";
import {
  useThumbnailCache,
} from "../hooks/useThumbnailCache";
import { buildHistoryQuery } from "../utils/historyQuery";
import { useVisiblePolling } from "../hooks/useVisiblePolling";
import { useRuntimeSnapshots } from "../hooks/useRuntimeSnapshots";
import {
  DATE_FILTER_OPTIONS,
  dateBounds,
  dateInputValue,
  localCalendarContextKey,
  type DateFilter,
} from "../utils/historyFilters";
import { LatestRequest } from "../utils/asyncControl";
import {
  historyPageCount,
  normalizeHistoryPage,
} from "../utils/historyPagination";
import { CalendarDays, Filter } from "lucide-react";
import {
  CATEGORY_FILTERS,
  CATEGORY_ICONS,
  MAX_CACHED_THUMBNAILS,
  NEW_GLOW_DURATION_MS,
  PAGE_SIZE,
  RESTORE_FEEDBACK_DURATION_MS,
  RUNTIME_WAIT_MS,
  SEARCH_DEBOUNCE_MS,
  SETTINGS_POLL_MS,
} from "./history/HistoryConstants";
import {
  isEditableTarget,
  persistHistoryAlwaysOnTop,
  readStoredHistoryAlwaysOnTop,
  resolvePreviewEntryId,
} from "./history/HistoryEntryHelpers";
import type { FilterOption } from "./history/HistoryViewTypes";
import { HistoryHeader } from "./history/HistoryHeader";
import { HistoryMainContent } from "./history/HistoryMainContent";
import { HistoryFooter } from "./history/HistoryFooter";

/* ── Types ──────────────────────────────────────────────────────── */

/* ── Constants ──────────────────────────────────────────────────── */

interface HistoryProps {
  collection?: HistoryCollection;
}

function logicalHistoryItemKey(entry: HistoryEntry): string {
  return entry.batch_id ? `batch:${entry.batch_id}` : `entry:${entry.id}`;
}

export function History({ collection = "all" }: HistoryProps) {
  const isFavoritesCollection = collection === "favorites";
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const favoriteMutations = useRef(new Map<string, boolean>());
  const [totalEntries, setTotalEntries] = useState<number | null>(0);
  const [hasMoreEntries, setHasMoreEntries] = useState(false);
  const [capabilities, setCapabilities] = useState<HistoryCapabilities | null>(null);
  const [migrationDiagnostics, setMigrationDiagnostics] = useState<MigrationDiagnostics | null>(null);
  const { thumbnails, loadThumbnail, retain: retainThumbnails, clear: clearThumbnails } =
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
  const [windowAlwaysOnTop, setWindowAlwaysOnTop] = useState(false);
  const [windowAlwaysOnTopPending, setWindowAlwaysOnTopPending] = useState(
    !isFavoritesCollection,
  );
  const closeHistory = useCallback(async () => {
    try {
      await closePreviewWindow(collection);
    } catch (error) {
      console.error("Could not close the preview window:", error);
    }
    try {
      await (isFavoritesCollection ? closeFavoritesWindow() : closeHistoryWindow());
    } catch (error) {
      console.error("Could not release the history window:", error);
    }
  }, [collection, isFavoritesCollection]);
  useEffect(() => () => {
    newGlowTimers.current.forEach(window.clearTimeout);
    newGlowTimers.current.clear();
  }, []);
  useEffect(() => {
    retainThumbnails(new Set(entries.map((entry) => entry.id)));
  }, [entries, retainThumbnails]);
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

  const { theme, themeAssetSlots } = useTheme();
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
        collection,
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
      const reconciledEntries = result.entries.map((entry) => {
        const pendingFavorite = favoriteMutations.current.get(logicalHistoryItemKey(entry));
        return pendingFavorite === undefined
          ? entry
          : { ...entry, pinned: pendingFavorite };
      });
      setEntries(reconciledEntries);
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
    collection,
  ]);

  /* ── Polling ──────────────────────────────────────────────────── */

  useEffect(() => {
    void loadCapabilities();
    void loadMigrationDiagnostics();
  }, [loadCapabilities, loadMigrationDiagnostics]);

  useVisiblePolling(loadSettings, SETTINGS_POLL_MS);
  useRuntimeSnapshots(async (snapshot) => {
    if (snapshot.history_version !== lastVersion.current) {
      lastVersion.current = snapshot.history_version;
      await loadHistory();
    }
    if (snapshot.sync_warning) {
      const key = {
        expired_event: "history.syncExpired",
        delivery_stalled: "history.syncStalled",
        delivery_shutdown: "history.syncShutdown",
        delivery_expired: "history.syncDeliveryExpired",
      }[snapshot.sync_warning.kind];
      if (key) {
        flashSyncWarning(t(key).replace("{peer}", snapshot.sync_warning.peer));
      }
    }
    for (const notification of snapshot.notifications ?? []) {
      if (notification.level === "error") {
        flashSyncWarning(notification.message);
      }
    }
    if (!progressBarEnabled) {
      setFileProgress(null);
      return;
    }
    setFileProgress(snapshot.progress?.active ? snapshot.progress : null);
  }, RUNTIME_WAIT_MS);

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
    if (isFavoritesCollection) return;
    const appWindow = getCurrentWindow();
    let disposed = false;
    const stored = readStoredHistoryAlwaysOnTop();
    const syncAlwaysOnTop = async () => {
      try {
        let actual = await appWindow.isAlwaysOnTop();
        if (!disposed) setWindowAlwaysOnTop(actual);
        const desired = stored ?? actual;
        if (stored !== null && actual !== desired) {
          await appWindow.setAlwaysOnTop(desired);
          actual = desired;
        }
        if (!disposed) setWindowAlwaysOnTop(actual);
      } catch (error) {
        if (!disposed) console.error("Could not sync history-window always-on-top state:", error);
      } finally {
        if (!disposed) setWindowAlwaysOnTopPending(false);
      }
    };
    void syncAlwaysOnTop();
    return () => {
      disposed = true;
    };
  }, [isFavoritesCollection]);

  const toggleWindowAlwaysOnTop = useCallback(async () => {
    if (windowAlwaysOnTopPending) return;
    const next = !windowAlwaysOnTop;
    setWindowAlwaysOnTopPending(true);
    try {
      await getCurrentWindow().setAlwaysOnTop(next);
      setWindowAlwaysOnTop(next);
      persistHistoryAlwaysOnTop(next);
    } catch (error) {
      console.error("Could not change history-window always-on-top state:", error);
    } finally {
      setWindowAlwaysOnTopPending(false);
    }
  }, [windowAlwaysOnTop, windowAlwaysOnTopPending]);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let disposed = false;
    const syncMinimized = () => {
      void appWindow.isMinimized()
        .then((minimized) => {
          if (!disposed) return syncPreviewWindowMinimized(minimized, collection);
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
      void closeHistory();
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
  }, [closeHistory, collection]);

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
        collection,
      ).catch((error: unknown) => {
        console.error("Could not open the preview window:", error);
        showActionError();
      });
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [collection, entries, expandedBatches, focusedId, showActionError]);

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
    const entry = entries.find((candidate) => candidate.id === id);
    if (entry && favoriteMutations.current.has(logicalHistoryItemKey(entry))) return;
    try {
      if (isFavoritesCollection) {
        await deleteFavoriteEntry(id);
      } else {
        await deleteEntry(id);
      }
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
      await closePreviewWindow(collection);
      clearThumbnails();
      setPage(0);
      setFocusedId(null);
      clearSelectedId();
      setExpandedBatches(new Set());
      setShowClearConfirm(false);
      prevIds.current = new Set();
      lastQueryKey.current = "";
      await Promise.all([
        loadHistory({ detectNewEntries: false }),
        loadMigrationDiagnostics(),
      ]);
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

  const handleFavoriteChange = async (entry: HistoryEntry) => {
    const mutationKey = logicalHistoryItemKey(entry);
    if (favoriteMutations.current.has(mutationKey)) return;
    const favorite = !entry.pinned;
    favoriteMutations.current.set(mutationKey, favorite);
    setEntries((current) => current.map((item) =>
      logicalHistoryItemKey(item) === mutationKey
        ? { ...item, pinned: favorite }
        : item
    ));
    try {
      const mutation = await setHistoryFavorite(entry.id, favorite);
      const nextFavorite = mutation?.favorite ?? favorite;
      if (isFavoritesCollection && !nextFavorite) {
        await loadHistory({ detectNewEntries: false });
      } else {
        const affectedIds = new Set(mutation?.affected_ids ?? [entry.id]);
        setEntries((current) => current.map((item) =>
          affectedIds.has(item.id) ? { ...item, pinned: nextFavorite } : item
        ));
      }
    } catch (error) {
      setEntries((current) => current.map((item) =>
        logicalHistoryItemKey(item) === mutationKey
          ? { ...item, pinned: entry.pinned }
          : item
      ));
      console.error("Favorite update failed:", error);
      showActionError();
    } finally {
      favoriteMutations.current.delete(mutationKey);
    }
  };

  const handleFavoriteProtected = useCallback(() => {
    flashActionError(t("history.favoriteProtected"));
  }, [flashActionError, t]);

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
    ? entries.find((entry) => entry.id === selectedId) ?? null
    : null;

  /* ── Render ───────────────────────────────────────────────────── */

  return (
    <div
      className={`app ${theme}`}
      data-focused-entry-id={focusedId === null ? undefined : String(focusedId)}
    >
      <HistoryHeader
        t={t}
        windowAlwaysOnTop={windowAlwaysOnTop}
        windowAlwaysOnTopPending={windowAlwaysOnTopPending}
        toggleWindowAlwaysOnTop={toggleWindowAlwaysOnTop}
        closeHistory={closeHistory}
        openFavorites={openFavoritesWindow}
        isFavoritesCollection={isFavoritesCollection}
        keywordDraft={keywordDraft}
        setKeywordDraft={setKeywordDraft}
        totalEntries={totalEntries}
        setShowClearConfirm={setShowClearConfirm}
        selectedCategory={selectedCategory}
        setSelectedCategory={setSelectedCategory}
        categoryOptions={categoryOptions}
        selectedDateFilter={selectedDateFilter}
        dateOptions={dateOptions}
        handleDateFilterChange={handleDateFilterChange}
        dateRangeFilterEnabled={capabilities?.date_range_filter ?? true}
        customStartDate={customStartDate}
        customEndDate={customEndDate}
        setCustomStartDate={setCustomStartDate}
        setCustomEndDate={setCustomEndDate}
        activeDateBounds={activeDateBounds}
        hasActiveFilters={hasActiveFilters}
        migrationDiagnostics={migrationDiagnostics}
      />

      <HistoryMainContent
        t={t}
        themeAssetSlots={themeAssetSlots}
        loading={loading}
        entries={entries}
        hasActiveFilters={hasActiveFilters}
        calendarNow={calendarNow}
        expandedBatches={expandedBatches}
        newIds={newIds}
        selectedId={selectedId}
        focusedId={focusedId}
        pageAnimationRevision={pageAnimationRevision}
        thumbnails={thumbnails}
        loadThumbnail={loadThumbnail}
        setFocusedId={setFocusedId}
        setExpandedBatches={setExpandedBatches}
        handleRestore={handleRestore}
        handleRestoreBatch={handleRestoreBatch}
        handleDelete={handleDelete}
        handleFavoriteChange={handleFavoriteChange}
        handleFavoriteProtected={handleFavoriteProtected}
        collection={collection}
        isFavoritesCollection={isFavoritesCollection}
      />

      <HistoryFooter
        t={t}
        entriesLength={entries.length}
        hasPrev={hasPrev}
        hasNext={hasNext}
        page={page}
        totalEntries={totalEntries}
        totalPages={totalPages}
        setPage={setPage}
        progressBarEnabled={progressBarEnabled}
        fileProgress={fileProgress}
        handleCancelFileBatch={handleCancelFileBatch}
        actionError={actionError}
        syncWarning={syncWarning}
        restoredEntry={restoredEntry}
        showClearConfirm={showClearConfirm}
        clearing={clearing}
        setShowClearConfirm={setShowClearConfirm}
        handleClearHistory={handleClearHistory}
        isFavoritesCollection={isFavoritesCollection}
      />
    </div>
  );
}

export function Favorites() {
  return <History collection="favorites" />;
}
