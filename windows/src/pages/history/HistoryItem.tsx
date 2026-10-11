import { useId, useRef, useState } from "react";
import type { CSSProperties } from "react";
import {
  ChevronDown,
  CircleEllipsis,
  FileText,
  HelpCircle,
  Info,
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
import {
  explicitRestoreFormats,
  rowDiagnostics,
  rowStatusLabel,
  showsAdvancedActions,
  showsMaintenanceActions,
} from "./historyRowActions";
import { LazyThumbnail } from "./HistoryThumbnail";
import { HistoryRowPopover } from "./HistoryRowPopover";
import type { HistoryCategory, HistoryEntry } from "../../tailsyncClient";
import { thumbnailKeyForEntry, type ThumbnailData } from "../../hooks/useThumbnailCache";
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
  thumbnails: Map<string, ThumbnailData>;
  loadThumbnail: (cacheKey: string, id: number) => void;
  setFocusedId: (id: number) => void;
  handleRestore: (id: number, format?: string) => Promise<void>;
  handleArchiveImages: (id: number) => Promise<void>;
  handleAuthorizeSource: (id: number) => Promise<void>;
  handleImportSourcePacket: (id: number) => Promise<void>;
  handleRevokeSource: () => Promise<void>;
  canRevokeSource: boolean;
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
  handleArchiveImages,
  handleAuthorizeSource,
  handleImportSourcePacket,
  handleRevokeSource,
  canRevokeSource,
  handleDelete,
  handleFavoriteChange,
  handleFavoriteProtected,
  isFavoritesCollection,
}: HistoryItemProps) {
  const displayKind = entry.display?.kind ?? (entry.type === "composite" ? "unknown" : entry.type);
  const CategoryIcon = displayKind === "document" ? FileText : displayKind === "unknown" ? HelpCircle : CATEGORY_ICONS[category];
  const thumbnailKey = thumbnailKeyForEntry(entry, isFavoritesCollection ? "favorites" : "all");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const diagnosticsId = useId();
  const diagnosticsToggle = useRef<HTMLButtonElement>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const advancedId = useId();
  const [actionsOpen, setActionsOpen] = useState(false);
  const actionsId = useId();
  const actionsToggle = useRef<HTMLButtonElement>(null);
  const longPress = useLongPressFavorite(
    () => void handleFavoriteChange(entry),
    true,
    entry.pinned,
  );
  const rowStyle: FavoriteStyle = {
    animationDelay: isExpandedBatchReveal
      ? `${Math.min(batchPosition - COLLAPSED_BATCH_FILE_LIMIT, 3) * 12}ms`
      : isPageEnterItem
        ? `${enterIndex * 20}ms`
        : undefined,
    "--favorite-progress": longPress.progress,
  };
  const statusLabel = rowStatusLabel(entry.composite, t);
  const restoreFormats = entry.composite ? explicitRestoreFormats(entry.composite) : [];
  const maintenanceActions = entry.composite ? showsMaintenanceActions(entry.composite) : false;
  const advancedActions = entry.composite
    ? showsAdvancedActions(entry.composite, {
        authorize: Boolean(handleAuthorizeSource),
        importPacket: Boolean(handleImportSourcePacket),
        revoke: canRevokeSource,
      })
    : false;

  const showsActions = restoreFormats.length > 0 || maintenanceActions || advancedActions;

  return (
    <article
      className={[
        "history-item",
        entry.composite && "is-composite",
        isNew && "is-new",
        isSelected && "restored",
        isFocused && "focused",
        entry.pinned && "is-favorite",
        longPress.isCharging && "favorite-charging",
        longPress.isTriggered && "favorite-triggered",
        longPress.triggeredAction === "favorite" && "favorite-triggered-favorite",
        longPress.triggeredAction === "unfavorite" && "favorite-triggered-unfavorite",
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
      {(entry.display?.thumbnail_eligible ?? (entry.type === "image" || (entry.composite?.image_count ?? 0) > 0 || entry.composite?.native_only)) ? (
        <LazyThumbnail
          id={entry.id}
          cacheKey={thumbnailKey}
          data={thumbnails.get(thumbnailKey)}
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
            {displayKind === "document" || displayKind === "unknown" ? (
              <span className="item-type">{t(`history.display.${displayKind}`)}</span>
            ) : categories.map((label) => (
              <span className={`item-type ${label}`} key={label}>
                {t(`history.category.${label}`)}
              </span>
            ))}
          </span>
          <span className="item-time">{formatTime(entry.timestamp)}</span>
          <span className="item-peer">
            {entry.source_peer === "self" ? t("history.peer.self") : entry.source_peer}
          </span>
        </div>
        <div className="item-desc" title={entry.description}>
          {entry.type === "composite" ? entry.description.trim() : entry.description}
        </div>
        <div className="item-footer">
          <span className="item-size">{formatSize(entry.size_bytes)}</span>
          {statusLabel && <span className="item-composite-status">{statusLabel}</span>}
          <span className="item-row-actions">
          {entry.composite && (
            <button type="button" className="item-action-button item-details-toggle"
              ref={diagnosticsToggle}
              id={`${diagnosticsId}-toggle`}
              aria-expanded={diagnosticsOpen}
              aria-controls={diagnosticsOpen ? diagnosticsId : undefined}
              aria-label={t("history.composite.details")}
              title={t("history.composite.details")}
              onContextMenu={(event) => event.stopPropagation()}
              onClick={() => { setActionsOpen(false); setDiagnosticsOpen(open => !open); }}>
              <Info size={17} strokeWidth={1.8} aria-hidden="true" />
            </button>
          )}
          {showsActions && (
            <button type="button" className="item-action-button item-more-toggle"
              ref={actionsToggle}
              aria-expanded={actionsOpen}
              aria-controls={actionsOpen ? actionsId : undefined}
              aria-label={t("history.composite.moreActions")}
              title={t("history.composite.moreActions")}
              onContextMenu={event => event.stopPropagation()}
              onClick={() => { setDiagnosticsOpen(false); setAdvancedOpen(false); setActionsOpen(open => !open); }}>
              <CircleEllipsis size={17} strokeWidth={1.8} aria-hidden="true" />
              <ChevronDown size={11} strokeWidth={1.8} aria-hidden="true" />
            </button>
          )}
          </span>
          <span className="favorite-stamp" aria-hidden="true">
            <Star size={13} fill="currentColor" />
          </span>
        </div>
        {showsActions && actionsOpen && (
          <HistoryRowPopover id={actionsId} anchor={actionsToggle} kind="actions"
            label={t("history.composite.moreActions")} onDismiss={() => { setActionsOpen(false); setAdvancedOpen(false); }}>
            {restoreFormats.map(format => (
              <button key={format} type="button" className="item-copy-format"
                onClick={() => { setActionsOpen(false); void handleRestore(entry.id, format); }}
                aria-label={t(`history.composite.copy.${format}`)}>
                {t(`history.composite.copy.${format}`)}
              </button>
            ))}
            {maintenanceActions && (
              <button type="button" className="item-copy-format"
                onClick={() => { setActionsOpen(false); void handleArchiveImages(entry.id); }}
                aria-label={t("history.composite.archiveImages")}
                title={t("history.composite.archiveImagesHint")}>
                {t("history.composite.archiveImages")}
              </button>
            )}
            {advancedActions && (
              <button type="button" className="item-copy-format item-advanced-toggle"
                id={`${advancedId}-toggle`}
                aria-expanded={advancedOpen}
                aria-controls={advancedOpen ? advancedId : undefined}
                aria-label={t("history.composite.advanced")}
                onClick={() => setAdvancedOpen(open => !open)}>
                {t("history.composite.advanced")}
                <ChevronDown size={13} aria-hidden="true" />
              </button>
            )}
            {advancedActions && advancedOpen && <div className="item-advanced-actions" id={advancedId}>
            {maintenanceActions && <button type="button" className="item-copy-format"
              onClick={() => void handleAuthorizeSource(entry.id)}
              aria-label={t("history.composite.authorizeSource")}>
              {t("history.composite.authorizeSource")}
            </button>}
            {maintenanceActions && <button type="button" className="item-copy-format"
              onClick={() => void handleImportSourcePacket(entry.id)}
              aria-label={t("history.composite.importSourcePacket")}>
              {t("history.composite.importSourcePacket")}
            </button>}
            {canRevokeSource && (
              <button type="button" className="item-copy-format"
                onClick={() => void handleRevokeSource()}
                aria-label={t("history.composite.revokeSource")}>
                {t("history.composite.revokeSource")}
              </button>
            )}
            </div>}
          </HistoryRowPopover>
        )}
        {entry.composite && diagnosticsOpen && (
          <HistoryRowPopover id={diagnosticsId} anchor={diagnosticsToggle} kind="details"
            label={t("history.composite.details")} labelledBy={`${diagnosticsId}-toggle`}
            onDismiss={() => setDiagnosticsOpen(false)}>
            <div className="item-diagnostics-title">{t("history.composite.details")}</div>
            <dl className="item-diagnostics" tabIndex={0}>
            {rowDiagnostics(entry.composite, t).map(row => (
              <div className="item-diagnostics-row" key={row.label}>
                <dt>{row.label}</dt>
                <dd>{row.value}</dd>
              </div>
            ))}
            </dl>
          </HistoryRowPopover>
        )}
      </div>
    </article>
  );
}
