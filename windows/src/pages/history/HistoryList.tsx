import {
  ArrowLeftRight,
  ChevronDown,
  ChevronUp,
  Clipboard,
  Folder,
  Pin,
} from "lucide-react";
import {
  CATEGORY_ICONS,
  COLLAPSED_BATCH_FILE_LIMIT,
  MAX_PAGE_ENTER_ITEMS,
} from "./HistoryConstants";
import {
  formatSize,
  formatTime,
  resolvedCategories,
} from "./HistoryEntryHelpers";
import { LazyThumbnail } from "./HistoryThumbnail";
import {
  computeBatchInfos,
  groupEntriesByDate,
  GROUP_LABEL_KEYS,
} from "../../utils/historyGrouping";
import type { HistoryListProps } from "./HistoryViewTypes";

export function HistoryList({
  t,
  entries,
  calendarNow,
  expandedBatches,
  newIds,
  selectedId,
  focusedId,
  pageAnimationRevision,
  thumbnails,
  loadThumbnail,
  setFocusedId,
  setExpandedBatches,
  handleRestore,
  handleRestoreBatch,
  handleDelete,
  handlePinnedChange,
}: HistoryListProps) {
  const orderedGroups = groupEntriesByDate(entries, calendarNow);
  const batchInfos = computeBatchInfos(orderedGroups);
  let pageEnterIndex = 0;

  return (
    <div className="history-list">
      <div className="history-page" key={pageAnimationRevision}>
        {orderedGroups.map(([group, groupEntries]) => (
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
                      if ((event.target as HTMLElement).closest("button, a, [role='button']")) return;
                      setFocusedId(entry.id);
                      event.currentTarget.focus({ preventScroll: true });
                    }}
                    onDoubleClick={() => void handleRestore(entry.id)}
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
                      <div className="item-desc" title={entry.description}>
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
        ))}
      </div>
    </div>
  );
}
