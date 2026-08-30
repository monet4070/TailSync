import type { CSSProperties } from "react";
import {
  ArrowLeftRight,
  Star,
} from "lucide-react";
import { useLongPressFavorite } from "../../hooks/useLongPressFavorite";
import {
  COLLAPSED_BATCH_FILE_LIMIT,
  CATEGORY_ICONS,
} from "./HistoryConstants";
import {
  formatSize,
  formatTime,
} from "./HistoryEntryHelpers";
import { LazyThumbnail } from "./HistoryThumbnail";
import type { HistoryCategory, HistoryEntry } from "../../tailsyncClient";
import type { ThumbnailData } from "../../hooks/useThumbnailCache";
import type { Translate } from "./HistoryViewTypes";

interface HistoryItemProps {
  t: Translate;
  entry: HistoryEntry;
  categories: HistoryCategory[];
  category: HistoryCategory;
  isNew: boolean;
  isSelected: boolean;
  isFocused: boolean;
  isExpandedBatchReveal: boolean;
  isPageEnterItem: boolean;
  enterIndex: number;
  batchPosition: number;
  thumbnails: Map<number, ThumbnailData>;
  loadThumbnail: (id: number) => void;
  setFocusedId: (id: number) => void;
  handleRestore: (id: number) => Promise<void>;
  handleDelete: (id: number) => Promise<void>;
  handleFavoriteChange: (entry: HistoryEntry) => Promise<void>;
  handleFavoriteProtected: () => void;
  isFavoritesCollection: boolean;
}

type FavoriteStyle = CSSProperties & { "--favorite-progress"?: number };

export function HistoryItem({
  t,
  entry,
  categories,
  category,
  isNew,
  isSelected,
  isFocused,
  isExpandedBatchReveal,
  isPageEnterItem,
  enterIndex,
  batchPosition,
  thumbnails,
  loadThumbnail,
  setFocusedId,
  handleRestore,
  handleDelete,
  handleFavoriteChange,
  handleFavoriteProtected,
  isFavoritesCollection,
}: HistoryItemProps) {
  const CategoryIcon = CATEGORY_ICONS[category];
  const longPress = useLongPressFavorite(
    () => void handleFavoriteChange(entry),
  );
  const rowStyle: FavoriteStyle = {
    animationDelay: isExpandedBatchReveal
      ? `${Math.min(batchPosition - COLLAPSED_BATCH_FILE_LIMIT, 3) * 12}ms`
      : isPageEnterItem
        ? `${enterIndex * 20}ms`
        : undefined,
    "--favorite-progress": longPress.progress,
  };

  return (
    <article
      className={[
        "history-item",
        isNew && "is-new",
        isSelected && "restored",
        isFocused && "focused",
        entry.pinned && "is-favorite",
        longPress.isCharging && "favorite-charging",
        longPress.isTriggered && "favorite-triggered",
        isExpandedBatchReveal && "batch-expanded-item",
        isPageEnterItem && "page-enter-item",
      ].filter(Boolean).join(" ")}
      style={rowStyle}
      data-id={entry.id}
      data-focused={isFocused ? "true" : undefined}
      tabIndex={0}
      aria-selected={isFocused}
      onPointerDown={longPress.onPointerDown}
      onPointerMove={longPress.onPointerMove}
      onPointerUp={longPress.onPointerUp}
      onPointerCancel={longPress.onPointerCancel}
      onClick={(event) => {
        if (longPress.suppressClick()) return;
        if ((event.target as HTMLElement).closest("button, a, [role='button']")) return;
        setFocusedId(entry.id);
        event.currentTarget.focus({ preventScroll: true });
      }}
      onDoubleClick={(event) => {
        if (longPress.suppressClick()) return;
        if ((event.target as HTMLElement).closest("button, a, [role='button']")) return;
        void handleRestore(entry.id);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        const completedActivePress = longPress.suppressContextMenu();
        longPress.cancel();
        if (completedActivePress) return;
        if (entry.pinned && !isFavoritesCollection) {
          handleFavoriteProtected();
          return;
        }
        void handleDelete(entry.id);
      }}
    >
      <span className="favorite-press-progress" aria-hidden="true" />
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
          <span className="item-time">{formatTime(entry.timestamp)}</span>
          <span className="item-peer">
            <ArrowLeftRight className="item-peer-icon" size={11} strokeWidth={1.8} aria-hidden="true" />
            {entry.source_peer}
          </span>
          {(entry.pinned || longPress.isTriggered) && (
            <span className="favorite-stamp" aria-hidden="true">
              <Star size={13} fill="currentColor" />
            </span>
          )}
        </div>
        <div className="item-desc" title={entry.description}>
          {entry.description}
        </div>
        <div className="item-footer">
          <span className="item-size">{formatSize(entry.size_bytes)}</span>
          {longPress.isTriggered && (
            <button
              className={`pin-entry${entry.pinned ? " active" : ""}`}
              type="button"
              title={entry.pinned ? t("history.unpin") : t("history.pin")}
              aria-label={entry.pinned ? t("history.unpin") : t("history.pin")}
              aria-pressed={entry.pinned}
              onClick={(event) => {
                event.stopPropagation();
                void handleFavoriteChange(entry);
              }}
            >
              <Star size={12} fill={entry.pinned ? "currentColor" : "none"} aria-hidden="true" />
            </button>
          )}
        </div>
      </div>
    </article>
  );
}
