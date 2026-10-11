import { useRestoreActivity } from "../hooks/useRestoreActivity";
import { StableIpcError } from "../utils/stableIpcError";
import { useState, useEffect, useLayoutEffect, useCallback, useRef, useMemo } from "react";
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
  archiveEntryImages,
  sourceContextGrantCreate,
  sourceContextGrantRevoke,
  sourceContextImport,
  sourceContextExportTicket,
  restoreFileBatch,
  setHistoryFavorite,
  type FileProgress,
  type HistoryCapabilities,
  type HistoryCategory,
  type HistoryCollection,
  type HistoryEntry,
  type MigrationDiagnostics,
  type SyncWarning,
} from "../tailsyncClient";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { HistoryImeGuard, visibleHistoryIds } from "../utils/historyKeyboard";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTheme } from "../hooks/useTheme";
import { useI18n } from "../hooks/useI18n";
import { useTransient } from "../hooks/useTransient";
import { useHistoryNotice, type HistoryNotice } from "../hooks/useHistoryNotice";
import { useSyncWarningNotice } from "../hooks/useSyncWarningNotice";
import {
  thumbnailKeyForEntry,
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
  MAX_CACHED_THUMBNAIL_BYTES,
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
  const deleting = useRef(false);
  const [totalEntries, setTotalEntries] = useState<number | null>(0);
  const [hasMoreEntries, setHasMoreEntries] = useState(false);
  const [capabilities, setCapabilities] = useState<HistoryCapabilities | null>(null);
  const [migrationDiagnostics, setMigrationDiagnostics] = useState<MigrationDiagnostics | null>(null);
  const { thumbnails, loadThumbnail, retain: retainThumbnails, clear: clearThumbnails } =
    useThumbnailCache(MAX_CACHED_THUMBNAILS, MAX_CACHED_THUMBNAIL_BYTES / 2);
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
  const [historyNotice, showHistoryNotice, clearHistoryNotice] = useHistoryNotice();
  const [failedRestore, setFailedRestore] = useState<{
    id: number; format?: string; type: string; dataHash: string; identity: import("../types/localContracts.generated").HistoryIdentity | null; message: string;
  } | null>(null);
  const renderedHistoryNotice = useMemo<HistoryNotice | null>(() => failedRestore
    ? { key: "restore-failed", level: "error", message: failedRestore.message, occurrences: 1 }
    : historyNotice, [failedRestore, historyNotice]);
  const [syncWarning, setSyncWarning] = useState<SyncWarning | null>(null);
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
  const [lastSourceGrantId, setLastSourceGrantId] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const ime = useRef(new HistoryImeGuard());
  const restoring = useRef(false);
  const sessionActive = useRef(true);
  const archiveRequestGeneration = useRef(0);
  const previewOpen = useRef(false);
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);
  const previewRequestKey = useRef("");
  const previewQueue = useRef(Promise.resolve());
  const previewGeneration = useRef(0);
  const { activeRef: externalRestoring } = useRestoreActivity(() => {
    previewGeneration.current++;
    previewRequestKey.current = "";
  });
  const pendingPageEdge = useRef<"first" | "last" | null>(null);
  const closePreview = useCallback(async () => {
    previewOpen.current = false;
    setIsPreviewOpen(false);
    previewGeneration.current++;
    previewRequestKey.current = "";
    await closePreviewWindow(collection);
  }, [collection]);
  const closeHistory = useCallback(async () => {
    sessionActive.current = false;
    archiveRequestGeneration.current++;
    try {
      await closePreview();
    } catch (error) {
      console.error("Could not close the preview window:", error);
    }
    try {
      await (isFavoritesCollection ? closeFavoritesWindow() : closeHistoryWindow());
    } catch (error) {
      console.error("Could not release the history window:", error);
    }
  }, [closePreview, isFavoritesCollection]);
  useEffect(() => () => {
    newGlowTimers.current.forEach(window.clearTimeout);
    newGlowTimers.current.clear();
  }, []);
  useEffect(() => {
    retainThumbnails(new Set(entries.map(entry => thumbnailKeyForEntry(entry, collection))));
  }, [collection, entries, retainThumbnails]);
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
  const syncWarningKey = syncWarning && {
    delivery_rejected: "history.deliveryRejected",
    expired_event: "history.syncExpired",
    composite_incomplete: "history.composite.syncIncomplete",
    composite_apply_unknown: "history.composite.applyUnknown",
    composite_partial: "history.composite.partial",
    composite_superseded: "history.composite.superseded",
    delivery_stalled: "history.syncStalled",
    delivery_shutdown: "history.syncShutdown",
    delivery_expired: "history.syncDeliveryExpired",
    // Capture failure remains visible independently of delivery.
    clipboard_unavailable: "history.clipboardUnavailable",
    capture_failed: "history.captureFailed",
    capture_origin_unknown: "history.captureOriginUnknown",
    capture_not_shared: "history.captureNotShared",
    warning_overflow: "history.warningOverflow",
    capture_not_saved: "history.captureNotSaved",
    receive_not_saved: "history.receiveNotSaved",
  }[syncWarning.kind];
  useSyncWarningNotice(syncWarning,
    syncWarningKey ? t(syncWarningKey).replace("{peer}", syncWarning!.peer) : null,
    renderedHistoryNotice, showHistoryNotice);
  const historyLoadErrorMessage = t("history.loadError");
  const showActionError = useCallback(() => {
    showHistoryNotice({
      key: "action-failed",
      level: "error",
      message: t("history.actionFailed"),
    });
  }, [t, showHistoryNotice]);

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

  const desiredQueryKey = JSON.stringify([keywordDraft, selectedCategory, selectedDateFilter,
    activeDateBounds, page, collection]);
  const desiredQuery = useRef(desiredQueryKey);
  useLayoutEffect(() => { desiredQuery.current = desiredQueryKey; }, [desiredQueryKey]);
  const loadedQuery = useRef("");
  const [loadedQueryKey, setLoadedQueryKey] = useState("");
  const focusSearch = useCallback(() => {
    searchRef.current?.focus({ preventScroll: true });
    searchRef.current?.select();
  }, []);

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
    const requestedQuery = desiredQueryKey;
    if (keyword !== keywordDraft) return;
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
      const result = await getHistoryPage(query, () => sessionActive.current && historyRequests.current.isCurrent(requestGeneration) && desiredQuery.current === requestedQuery);
      if (!historyRequests.current.isCurrent(requestGeneration) || desiredQuery.current !== requestedQuery) return;

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
      loadedQuery.current = requestedQuery;
      setLoadedQueryKey(requestedQuery);
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
        showHistoryNotice({
          key: "history-load",
          level: "error",
          message: historyLoadErrorMessage,
        });
      }
    } finally {
      if (historyRequests.current.isCurrent(requestGeneration)) {
        setLoading(false);
      }
    }
  }, [
    activeDateBounds,
    desiredQueryKey,
    keywordDraft,
    capabilities,
    keyword,
    page,
    selectedCategory,
    collection,
    showHistoryNotice,
    historyLoadErrorMessage,
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
    setSyncWarning(snapshot.sync_warning ?? null);
    for (const notification of snapshot.notifications ?? []) {
      if (notification.level === "error") {
        showHistoryNotice({
          key: `runtime-error:${notification.message}`,
          level: "error",
          message: notification.message,
        });
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
    pendingPageEdge.current = null;
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
      await syncPreviewWindowMinimized(false, collection);
    } catch (error) {
      console.error("Could not change history-window always-on-top state:", error);
    } finally {
      setWindowAlwaysOnTopPending(false);
    }
  }, [collection, windowAlwaysOnTop, windowAlwaysOnTopPending]);

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

  const visibleIds = useMemo(() => visibleHistoryIds(entries, expandedBatches, calendarNow),
    [entries, expandedBatches, calendarNow]);
  const visibleIdsRef = useRef<number[]>([]);
  useLayoutEffect(() => { visibleIdsRef.current = visibleIds; }, [visibleIds]);
  const queryReady = keyword === keywordDraft && loadedQueryKey === desiredQueryKey;

  useEffect(() => {
    if (!queryReady) return;
    setFocusedId((current) => {
      const edge = pendingPageEdge.current;
      pendingPageEdge.current = null;
      if (edge) return (edge === "last" ? visibleIds.at(-1) : visibleIds[0]) ?? null;
      return current !== null && visibleIds.includes(current) ? current : visibleIds[0] ?? null;
    });
  }, [visibleIds, queryReady]);

  useEffect(() => {
    // Scroll only the minimum needed to keep the row visible, smoothly, so the
    // list glides instead of jumping. Once the row reaches an edge it stays put
    // and the list slides under it.
    rootRef.current
      ?.querySelector<HTMLElement>(`[data-id="${focusedId}"]`)
      ?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  }, [focusedId]);

  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const subscribe = (name: string, callback: () => void) => {
      void listen(name, () => { if (!disposed) callback(); }).then((stop) => {
        if (disposed) stop(); else stops.push(stop);
      }).catch(console.error);
    };
    subscribe("tailsync://history-open", () => {
      sessionActive.current = true;
      archiveRequestGeneration.current++;
      setFocusedId(visibleIdsRef.current[0] ?? null);
      focusSearch();
    });
    subscribe("tailsync://preview-close", () => {
      previewOpen.current = false;
      setIsPreviewOpen(false);
      previewGeneration.current++;
      previewRequestKey.current = "";
      if (sessionActive.current) {
        rootRef.current?.querySelector<HTMLElement>(".history-list")?.focus({ preventScroll: true });
      }
    });
    return () => { disposed = true; archiveRequestGeneration.current++; stops.forEach((stop) => stop()); };
  }, [focusSearch]);

  const showPreview = useCallback((takeFocus: boolean) => {
    if (!sessionActive.current || !queryReady || focusedId === null || restoring.current || externalRestoring.current) return;
    const targetId = resolvePreviewEntryId(focusedId, entries, expandedBatches);
    if (targetId === null) return;
    const batch = entries.find((entry) => entry.id === focusedId)?.batch_id ?? null;
    const batchId = batch && !expandedBatches.has(batch) ? batch : null;
    const key = JSON.stringify([targetId, batchId]);
    if (!takeFocus && previewOpen.current && previewRequestKey.current === key) return;
    previewOpen.current = true;
    setIsPreviewOpen(true);
    previewRequestKey.current = key;
    const generation = previewGeneration.current;
    // Serialize opens; skipped queued work cannot reopen after close/query replacement.
    previewQueue.current = previewQueue.current.then(async () => {
      if (generation !== previewGeneration.current || !previewOpen.current ||
          restoring.current || externalRestoring.current || !sessionActive.current || desiredQuery.current !== loadedQuery.current || previewRequestKey.current !== key) return;
      try {
        await openPreviewWindow(targetId, batchId, collection, takeFocus);
      } catch (error) {
        if (generation !== previewGeneration.current) return;
        console.error("Could not open preview:", error);
        showActionError();
      }
    });
  }, [collection, entries, expandedBatches, externalRestoring, focusedId, queryReady, showActionError]);

  useEffect(() => {
    if (!isPreviewOpen) return;
    if (!queryReady && pendingPageEdge.current !== null) return;
    if (!queryReady || focusedId === null) {
      void closePreview().catch(console.error);
    } else {
      showPreview(false);
    }
  }, [isPreviewOpen, focusedId, queryReady, closePreview, showPreview]);

  const restoreSelected = useCallback(async (id: number, keepOpen = true, format?: string, heldIdentity?: import("../types/localContracts.generated").HistoryIdentity | null, originalId?: number) => {
    if (deleting.current || restoring.current || externalRestoring.current || !sessionActive.current || !queryReady || !visibleIds.includes(id)) return;
    const entry = entries.find((entry) => entry.id === id);
    if (!entry) return;
    restoring.current = true;
    setFailedRestore(null);
    clearHistoryNotice();
    previewGeneration.current++;
    previewRequestKey.current = "";
    try {
      const report = await restoreEntry(id, format, heldIdentity ?? entry.composite?.identity, originalId ?? id);
      setFailedRestore(null);
      flashSelectedId(id);
      showHistoryNotice({ key: "restore-success", level: report?.complete === false ? "warning" : "success", message: report?.complete === false
        ? t("history.restore.availableContent").replace("{count}", String(report.missing_images))
        : t("history.restored") });
      if (!keepOpen && report?.complete !== false) await closeHistory();
    } catch (error) {
      console.error("Restore failed:", error);
      const key = error instanceof StableIpcError ? error.messageKey : null;
      if (key?.startsWith("history.restore.code.")) {
        const canRecopy = ["history.restore.code.publish_partial", "history.restore.code.publish_unknown"].includes(key);
        setFailedRestore(canRecopy ? {
          id, format, type: entry.type, dataHash: entry.data_hash, identity: entry.composite?.identity ?? null, message: t(key),
        } : null);
        showHistoryNotice({ key: "restore-failed", level: "error", message: t(key) });
      } else {
        setFailedRestore(null);
        showActionError();
      }
    } finally {
      restoring.current = false;
    }
  }, [entries, externalRestoring, queryReady, visibleIds, flashSelectedId, clearHistoryNotice, showHistoryNotice, t, closeHistory, showActionError]);

  const handleArchiveImages = async (id: number) => {
    if (deleting.current || restoring.current || externalRestoring.current || !sessionActive.current || !queryReady || !visibleIds.includes(id)) return;
    const entry = entries.find((candidate) => candidate.id === id);
    if (!entry) return;
    restoring.current = true;
    const generation = archiveRequestGeneration.current;
    clearHistoryNotice();
    try {
      const report = await archiveEntryImages(id);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      const failureMessages = report.failures.slice(0, 8).map(failure => {
        const reason = ["archive_not_http", "archive_user_info", "archive_not_public"].includes(failure.code)
          ? "archiveBlocked" : ["archive_decode", "archive_mime"].includes(failure.code)
          ? "archiveInvalid" : ["archive_too_many", "archive_too_large", "archive_total_budget"].includes(failure.code)
          ? "archiveLimit" : "archiveUnavailable";
        return `#${failure.image_index + 1}: ${t(`history.composite.${reason}`)}`;
      });
      showHistoryNotice({
        key: "archive-images",
        level: report.complete ? "success" : "error",
        message: report.complete
          ? t("history.composite.archiveDone")
          : [t("history.composite.archivePartial"), ...failureMessages].join(" · "),
      });
      await loadHistory({ detectNewEntries: false });
    } catch (error) {
      console.error("Archive images failed:", error);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      showActionError();
    } finally {
      restoring.current = false;
    }
  };

  const handleAuthorizeSource = async (id: number) => {
    if (deleting.current || restoring.current || externalRestoring.current || !sessionActive.current || !queryReady || !visibleIds.includes(id)) return;
    restoring.current = true;
    const generation = archiveRequestGeneration.current;
    clearHistoryNotice();
    try {
      const path = await save({ defaultPath: "tailsync-source-ticket.json" });
      if (!path || !sessionActive.current || archiveRequestGeneration.current !== generation) return;
      const report = await sourceContextGrantCreate(id);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) {
        await sourceContextGrantRevoke(report.grant_id);
        return;
      }
      try {
        await sourceContextExportTicket(path, report.ticket);
      } catch (exportError) {
        console.error("Source ticket export failed:", exportError);
        try {
          await sourceContextGrantRevoke(report.grant_id);
        } catch {
        }
        if (sessionActive.current && archiveRequestGeneration.current === generation) showActionError();
        return;
      }
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) {
        await sourceContextGrantRevoke(report.grant_id);
        return;
      }
      setLastSourceGrantId(report.grant_id);
      showHistoryNotice({
        key: "source-context",
        level: "success",
        message: t("history.composite.sourceAuthorized"),
      });
    } catch (error) {
      console.error("Source context grant failed:", error);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      showActionError();
    } finally {
      restoring.current = false;
    }
  };

  const handleImportSourcePacket = async (id: number) => {
    if (deleting.current || restoring.current || externalRestoring.current || !sessionActive.current || !queryReady || !visibleIds.includes(id)) return;
    restoring.current = true;
    const generation = archiveRequestGeneration.current;
    clearHistoryNotice();
    try {
      const picked = await open({ multiple: false });
      const path = typeof picked === "string" ? picked : null;
      if (!path || !sessionActive.current || archiveRequestGeneration.current !== generation) return;
      const report = await sourceContextImport(id, path);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      showHistoryNotice({
        key: "source-context",
        level: report.complete ? "success" : "error",
        message: report.complete
          ? t("history.composite.sourceImported")
          : t("history.composite.sourcePartial"),
      });
      await loadHistory({ detectNewEntries: false });
    } catch (error) {
      console.error("Source packet import failed:", error);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      showActionError();
    } finally {
      restoring.current = false;
    }
  };

  const handleRevokeSource = async () => {
    const grantId = lastSourceGrantId;
    if (!grantId) return;
    const generation = archiveRequestGeneration.current;
    try {
      const report = await sourceContextGrantRevoke(grantId);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      if (!report.revoked) { showActionError(); return; }
      setLastSourceGrantId(null);
      showHistoryNotice({
        key: "source-context",
        level: "success",
        message: t("history.composite.sourceRevoked"),
      });
    } catch (error) {
      console.error("Source context revoke failed:", error);
      showActionError();
    }
  };

  const retryFailedRestore = useCallback(() => {
    if (!failedRestore || !queryReady || restoring.current || externalRestoring.current) return;
    const original = entries.find((entry) => entry.id === failedRestore.id);
    // Deduplication can replace a row after an Unknown write. Follow only its
    // complete content identity; never recreate deleted content or guess by title.
    const identity = failedRestore.identity;
    const replacement = original
      ? original.type !== failedRestore.type ? undefined
        : identity ? original.composite?.identity?.version === 1 && identity.version === 1 &&
            original.composite.identity.capture_id === identity.capture_id && original.composite.identity.content_id === identity.content_id ? original : undefined
          : original.data_hash === failedRestore.dataHash && !original.composite?.identity ? original : undefined
      : identity?.version === 1 && identity.dedupe_key ? entries.find((entry) => entry.type === failedRestore.type &&
          entry.composite?.identity?.version === 1 && entry.composite.identity.content_id === identity.content_id &&
          entry.composite.identity.dedupe_key === identity.dedupe_key) : undefined;
    if (!replacement) {
      setFailedRestore(null);
      clearHistoryNotice();
      showHistoryNotice({ key: "restore-missing", level: "error", message: t("error.not_found") });
      return;
    }
    void restoreSelected(replacement.id, true, failedRestore.format, failedRestore.identity, failedRestore.id);
  }, [entries, failedRestore, queryReady, externalRestoring, clearHistoryNotice, showHistoryNotice, t, restoreSelected]);

  const handleDelete = useCallback(async (id: number) => {
    if (deleting.current || restoring.current || externalRestoring.current || !sessionActive.current || !queryReady) return;
    const entry = entries.find((candidate) => candidate.id === id);
    if (!entry || favoriteMutations.current.has(logicalHistoryItemKey(entry))) return;
    if (entry.pinned && !isFavoritesCollection) {
      showHistoryNotice({ key: "favorite-protected", level: "error", message: t("history.favoriteProtected") });
      return;
    }
    deleting.current = true;
    const generation = archiveRequestGeneration.current;
    const removedIds = new Set(entries.filter((candidate) => candidate.id === id ||
      (isFavoritesCollection && logicalHistoryItemKey(candidate) === logicalHistoryItemKey(entry))).map((candidate) => candidate.id));
    const index = visibleIds.indexOf(id);
    const nextId = visibleIds.slice(index + 1).find((candidate) => !removedIds.has(candidate)) ??
      visibleIds.slice(0, Math.max(index, 0)).reverse().find((candidate) => !removedIds.has(candidate)) ?? null;
    try {
      if (isFavoritesCollection) await deleteFavoriteEntry(id);
      else await deleteEntry(id);
      if (!sessionActive.current || archiveRequestGeneration.current !== generation) return;
      setFocusedId((current) => current !== null && removedIds.has(current) ? nextId : current);
      // Preserve page animation and avoid marking rows pulled from the next page as new.
      await Promise.all([
        loadHistory({ detectNewEntries: false }),
        loadMigrationDiagnostics(),
      ]);
    } catch (e) {
      console.error("Delete failed:", e);
      if (sessionActive.current && archiveRequestGeneration.current === generation) showActionError();
    } finally {
      deleting.current = false;
    }
  }, [entries, externalRestoring, isFavoritesCollection, loadHistory, loadMigrationDiagnostics,
    queryReady, showActionError, showHistoryNotice, t, visibleIds]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (deleting.current || restoring.current || externalRestoring.current || event.defaultPrevented || ime.current.blocks(event) || !sessionActive.current || showClearConfirm) return;
      if (event.altKey || event.metaKey) return;
      const search = event.target === searchRef.current;
      const editable = isEditableTarget(event.target);
      if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault(); focusSearch(); return;
      }
      if (editable && !search) return;
      // Preserve native activation of toolbar/filter buttons.
      if (event.target instanceof HTMLElement && event.target.closest("button, a, [role='button']")) return;
      if (event.ctrlKey && event.key !== "Enter") return;
      if (event.key === "Escape" && !event.repeat) {
        event.preventDefault();
        if (previewOpen.current) void closePreview().catch(console.error);
        else if (!restoring.current) void closeHistory();
        return;
      }
      if (!queryReady || restoring.current || loading) return;
      if (event.key === "Enter" && !event.shiftKey && !event.repeat && focusedId !== null) {
        event.preventDefault(); void restoreSelected(focusedId, event.ctrlKey); return;
      }
      if (event.shiftKey) return;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        if (search) {
          if (event.key === "ArrowDown" && visibleIds.length) {
            setFocusedId(visibleIds[0]);
            rootRef.current?.querySelector<HTMLElement>(".history-list")?.focus({ preventScroll: true });
          }
          return;
        }
        rootRef.current?.querySelector<HTMLElement>(".history-list")?.focus({ preventScroll: true });
        const index = focusedId === null ? -1 : visibleIds.indexOf(focusedId);
        if (event.key === "ArrowUp" && index <= 0 && !hasPrev) { focusSearch(); return; }
        const next = index + (event.key === "ArrowDown" ? 1 : -1);
        if (next >= 0 && next < visibleIds.length) setFocusedId(visibleIds[next]);
        else if ((next >= visibleIds.length && hasNext) || (next < 0 && hasPrev)) {
          pendingPageEdge.current = next < 0 ? "last" : "first";
          setPage(page + (next < 0 ? -1 : 1));
        }
        return;
      }
      if (search) return; // Preserve native search editing, including Delete and Space.
      if (event.key === "Delete" && !event.ctrlKey && !event.repeat && focusedId !== null) {
        event.preventDefault(); void handleDelete(focusedId); return;
      }
      if ((event.code === "Space" || event.key === " ") && !event.repeat) {
        event.preventDefault(); showPreview(false);
      } else if (event.key === "Tab" && previewOpen.current && !event.repeat) {
        event.preventDefault(); showPreview(true);
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [closeHistory, closePreview, focusSearch, focusedId, hasNext, hasPrev, loading, page,
    externalRestoring, handleDelete, queryReady, restoreSelected, showClearConfirm, showPreview, visibleIds]);

  /* ── Actions ──────────────────────────────────────────────────── */

  const handleRestore = (id: number, format?: string) => restoreSelected(id, true, format);

  const handleClearHistory = async () => {
    setClearing(true);
    try {
      await clearHistory();
      await closePreview();
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
    if (restoring.current || externalRestoring.current || !queryReady || !sessionActive.current) return;
    restoring.current = true;
    previewGeneration.current++;
    previewRequestKey.current = "";
    try {
      await restoreFileBatch(batchId);
      showHistoryNotice({
        key: "restore-success",
        level: "success",
        message: t("history.restored"),
      });
    } catch (error) {
      console.error("Batch restore failed:", error);
      showActionError();
    } finally {
      restoring.current = false;
    }
  };

  const handleFavoriteChange = async (entry: HistoryEntry) => {
    if (deleting.current) return;
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
    showHistoryNotice({
      key: "favorite-protected",
      level: "warning",
      message: t("history.favoriteProtected"),
    });
  }, [showHistoryNotice, t]);

  const handleCancelFileBatch = async (batchId: string) => {
    try {
      await cancelFileBatch(batchId);
    } catch (error) {
      console.error("Cancel file batch failed:", error);
      showActionError();
    }
  };

  const retryHistory = useCallback(() => {
    clearHistoryNotice();
    void loadHistory();
  }, [clearHistoryNotice, loadHistory]);

  /* ── Derived state ────────────────────────────────────────────── */

  /* ── Render ───────────────────────────────────────────────────── */

  return (
    <div
      ref={rootRef}
      onCompositionStart={() => ime.current.start()}
      onCompositionEnd={() => ime.current.end()}
      className={`app ${theme}`}
      data-focused-entry-id={focusedId === null ? undefined : String(focusedId)}
    >
      <HistoryHeader
        searchRef={searchRef}
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
        handleArchiveImages={handleArchiveImages}
        handleAuthorizeSource={handleAuthorizeSource}
        handleImportSourcePacket={handleImportSourcePacket}
        handleRevokeSource={handleRevokeSource}
        canRevokeSource={lastSourceGrantId !== null}
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
        showClearConfirm={showClearConfirm}
        clearing={clearing}
        setShowClearConfirm={setShowClearConfirm}
        handleClearHistory={handleClearHistory}
        isFavoritesCollection={isFavoritesCollection}
        historyNotice={renderedHistoryNotice}
        clearHistoryNotice={() => { setFailedRestore(null); clearHistoryNotice(); }}
        retryHistory={retryHistory}
        retryRestore={failedRestore ? retryFailedRestore : undefined}
      />
    </div>
  );
}

export function Favorites() {
  return <History collection="favorites" />;
}
