import { ChevronDown, ChevronUp, Clipboard, Folder } from "lucide-react";
import {
  COLLAPSED_BATCH_FILE_LIMIT,
  MAX_PAGE_ENTER_ITEMS,
} from "./HistoryConstants";
import { resolvedCategories } from "./HistoryEntryHelpers";
import {
  computeBatchInfos,
  groupEntriesByDate,
  GROUP_LABEL_KEYS,
} from "../../utils/historyGrouping";
import type { HistoryListProps } from "./HistoryViewTypes";
import { HistoryItem } from "./HistoryItem";

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
  handleFavoriteChange,
  handleFavoriteProtected,
  collection,
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
                  <HistoryItem
                    t={t}
                    entry={entry}
                    categories={categories}
                    category={category}
                    isNew={isNew}
                    isSelected={selectedId === entry.id}
                    isFocused={focusedId === entry.id}
                    isExpandedBatchReveal={isExpandedBatchReveal}
                    isPageEnterItem={isPageEnterItem}
                    enterIndex={enterIndex}
                    batchPosition={batchPosition}
                    thumbnails={thumbnails}
                    loadThumbnail={loadThumbnail}
                    setFocusedId={setFocusedId}
                    handleRestore={handleRestore}
                    handleDelete={handleDelete}
                    handleFavoriteChange={handleFavoriteChange}
                    handleFavoriteProtected={handleFavoriteProtected}
                    isFavoritesCollection={collection === "favorites"}
                  />
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
