import { Clipboard, SearchX, Star } from "lucide-react";
import { HistoryList } from "./HistoryList";
import type { HistoryMainContentProps } from "./HistoryViewTypes";

export function HistoryMainContent(props: HistoryMainContentProps) {
  return (
    <div data-tour="history-items" className="history-list" tabIndex={-1}>
      <HistoryMainContentBody {...props} />
    </div>
  );
}

function HistoryMainContentBody({
  shortcutNumbers,
  shortcutsActive,
  t,
  themeAssetSlots,
  loading,
  entries,
  hasActiveFilters,
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
  handleArchiveImages,
  handleAuthorizeSource,
  handleImportSourcePacket,
  handleRevokeSource,
  canRevokeSource,
  handleRestoreBatch,
  handleDelete,
  handleFavoriteChange,
  handleFavoriteProtected,
  collection,
  isFavoritesCollection,
}: HistoryMainContentProps) {
  if (loading && entries.length === 0) {
    return (
      <div className="skeleton-list">
        {[0, 1, 2, 3].map((index) => (
          <div className="skeleton-item" key={index}>
            <div className="skeleton-icon" />
            <div className="skeleton-lines">
              <div className="skeleton-line" />
              <div className="skeleton-line" />
              <div className="skeleton-line" />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className="empty-state">
        {hasActiveFilters ? (
          <>
            <div className="empty-state-illustration">
              <SearchX size={30} strokeWidth={1.35} aria-hidden="true" />
            </div>
            <div className="empty-state-title">{t("history.noMatches")}</div>
            <div className="empty-state-desc">{t("history.noMatchesDescription")}</div>
          </>
        ) : (
          <>
            <div className={`empty-state-illustration${!isFavoritesCollection && themeAssetSlots?.emptyState ? " has-theme-image" : ""}`}>
              {isFavoritesCollection ? (
                <Star size={30} strokeWidth={1.35} aria-hidden="true" />
              ) : !themeAssetSlots?.emptyState && (
                <Clipboard size={30} strokeWidth={1.35} aria-hidden="true" />
              )}
            </div>
            <div className="empty-state-title">
              {t(isFavoritesCollection ? "favorites.emptyTitle" : "history.emptyTitle")}
            </div>
            <div className="empty-state-desc">
              {t(isFavoritesCollection ? "favorites.emptyDescription" : "history.emptyDescription")}
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <HistoryList
      shortcutNumbers={shortcutNumbers}
      shortcutsActive={shortcutsActive}
      t={t}
      entries={entries}
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
      canRevokeSource={canRevokeSource}
      handleRestoreBatch={handleRestoreBatch}
      handleDelete={handleDelete}
      handleFavoriteChange={handleFavoriteChange}
      handleFavoriteProtected={handleFavoriteProtected}
      collection={collection}
    />
  );
}
