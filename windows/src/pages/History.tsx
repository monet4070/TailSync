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
import tailsyncIcon from "../../src-tauri/icons/32x32.png";

/* ── Types ──────────────────────────────────────────────────────── */

interface HistoryEntry {
  id: number;
  timestamp: string;
  type: "text" | "image" | "file";
  description: string;
  data_hash: string;
  size_bytes: number;
  source_peer: string;
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

/* ── Date grouping ──────────────────────────────────────────────── */

type DateGroup = "today" | "yesterday" | "thisWeek" | "thisMonth" | "older";

function getDateGroup(dateStr: string): DateGroup {
  const d = new Date(dateStr);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const itemDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());

  if (itemDate.getTime() === today.getTime()) return "today";
  if (itemDate.getTime() === yesterday.getTime()) return "yesterday";
  const diffDays = Math.floor(
    (today.getTime() - itemDate.getTime()) / (1000 * 60 * 60 * 24),
  );
  if (diffDays <= 7) return "thisWeek";
  if (diffDays <= 30) return "thisMonth";
  return "older";
}

function getGroupLabel(group: DateGroup, locale: string): string {
  const labels: Record<DateGroup, Record<string, string>> = {
    today: { en: "Today", "zh-CN": "今天" },
    yesterday: { en: "Yesterday", "zh-CN": "昨天" },
    thisWeek: { en: "This Week", "zh-CN": "这周" },
    thisMonth: { en: "This Month", "zh-CN": "本月" },
    older: { en: "Older", "zh-CN": "更早" },
  };
  return labels[group][locale] || labels[group]["en"];
}

const GROUP_ORDER: DateGroup[] = [
  "today",
  "yesterday",
  "thisWeek",
  "thisMonth",
  "older",
];

/* ── Helpers ────────────────────────────────────────────────────── */

function formatTime(iso: string) {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
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

  const { theme } = useTheme();
  const { t, locale } = useI18n();

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
    () =>
      keyword
        ? allEntries.filter(
            (e) =>
              e.description.toLowerCase().includes(keyword.toLowerCase()) ||
              e.type.toLowerCase().includes(keyword.toLowerCase()) ||
              e.source_peer.toLowerCase().includes(keyword.toLowerCase()),
          )
        : allEntries,
    [allEntries, keyword],
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
        keyword: keyword || null,
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
  }, [keyword, loadThumbnails]);

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
    loadHistory();
  }, [keyword, loadHistory]);

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

  /* ── Render helpers ───────────────────────────────────────────── */

  const typeIcon = (type: string) => {
    switch (type) {
      case "text":
        return "📝";
      case "image":
        return "🖼️";
      case "file":
        return "📎";
      default:
        return "📋";
    }
  };

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
          title="Close"
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
          title={locale === "zh-CN" ? "清空全部历史记录" : "Clear all history"}
          aria-label={locale === "zh-CN" ? "清空全部历史记录" : "Clear all history"}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 10v6M14 10v6" />
          </svg>
        </button>
      </div>

      {keyword && filtered.length > 0 && (
        <div className="search-results-count">
          {locale === "zh-CN"
            ? `找到 ${filtered.length} 条结果`
            : `${filtered.length} result${filtered.length > 1 ? "s" : ""} found`}
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
          {keyword ? (
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
                {locale === "zh-CN"
                  ? "没有找到匹配的内容"
                  : "No matching entries"}
              </div>
              <div className="empty-state-desc">
                {locale === "zh-CN"
                  ? `未找到包含 "${keyword}" 的内容`
                  : `No entries matching "${keyword}"`}
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
                {locale === "zh-CN"
                  ? "剪贴板历史为空"
                  : "No clipboard history"}
              </div>
              <div className="empty-state-desc">
                {locale === "zh-CN"
                  ? "跨设备复制内容后会自动出现在这里"
                  : "Copy content on any device — it will appear here"}
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
              const g = getDateGroup(entry.timestamp);
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
                    {getGroupLabel(group, locale)}
                  </div>
                  {groupEntries.map((entry) => {
                    const delay = itemIndex * 30;
                    itemIndex++;
                    const isNew = newIds.has(entry.id);
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
                          <div className="item-icon">
                            {typeIcon(entry.type)}
                          </div>
                        )}

                        <div className="item-content">
                          <div className="item-meta">
                            <span className={`item-type ${entry.type}`}>
                              {entry.type === "text"
                                ? "Text"
                                : entry.type === "image"
                                  ? "Image"
                                  : "File"}
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
                            {entry.type === "file" ? "📁 " : ""}
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
              {locale === "zh-CN" ? "清空全部历史记录？" : "Clear all history?"}
            </h2>
            <p>
              {locale === "zh-CN"
                ? "此操作会永久删除此设备上的所有剪贴板历史，且无法撤销。"
                : "This permanently deletes all clipboard history on this device and cannot be undone."}
            </p>
            <div className="confirm-dialog-actions">
              <button type="button" onClick={() => setShowClearConfirm(false)} disabled={clearing}>
                {locale === "zh-CN" ? "取消" : "Cancel"}
              </button>
              <button type="button" className="danger" onClick={handleClearHistory} disabled={clearing}>
                {clearing
                  ? locale === "zh-CN" ? "正在清空..." : "Clearing..."
                  : locale === "zh-CN" ? "全部清空" : "Clear all"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
