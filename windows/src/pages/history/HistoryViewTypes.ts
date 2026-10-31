import type { Dispatch, RefObject, SetStateAction } from "react";
import type { LucideIcon } from "lucide-react";
import type {
  FileProgress,
  HistoryCategory,
  HistoryCollection,
  HistoryEntry,
  MigrationDiagnostics,
} from "../../tailsyncClient";
import type { ThumbnailData } from "../../hooks/useThumbnailCache";
import type { HistoryNotice } from "../../hooks/useHistoryNotice";
import type { DateBounds, DateFilter } from "../../utils/historyFilters";

export type Translate = (key: string) => string;

export interface FilterOption {
  value: string;
  label: string;
  icon?: LucideIcon;
  category?: HistoryCategory;
}

export interface HistoryHeaderProps {
  searchRef: RefObject<HTMLInputElement | null>;
  t: Translate;
  windowAlwaysOnTop: boolean;
  windowAlwaysOnTopPending: boolean;
  toggleWindowAlwaysOnTop: () => Promise<void>;
  closeHistory: () => Promise<void>;
  openFavorites: () => Promise<void>;
  isFavoritesCollection: boolean;
  keywordDraft: string;
  setKeywordDraft: Dispatch<SetStateAction<string>>;
  totalEntries: number | null;
  setShowClearConfirm: Dispatch<SetStateAction<boolean>>;
  selectedCategory: "all" | HistoryCategory;
  setSelectedCategory: (value: "all" | HistoryCategory) => void;
  categoryOptions: FilterOption[];
  selectedDateFilter: DateFilter;
  dateOptions: FilterOption[];
  handleDateFilterChange: (value: string) => void;
  dateRangeFilterEnabled: boolean;
  customStartDate: string;
  customEndDate: string;
  setCustomStartDate: Dispatch<SetStateAction<string>>;
  setCustomEndDate: Dispatch<SetStateAction<string>>;
  activeDateBounds: DateBounds;
  hasActiveFilters: boolean;
  migrationDiagnostics: MigrationDiagnostics | null;
}

export interface HistoryListProps {
  shortcutNumbers: ReadonlyMap<number, number>;
  shortcutsActive: boolean;
  t: Translate;
  entries: HistoryEntry[];
  calendarNow: Date;
  expandedBatches: Set<string>;
  newIds: Set<number>;
  selectedId: number | null;
  focusedId: number | null;
  pageAnimationRevision: number;
  thumbnails: Map<string, ThumbnailData>;
  loadThumbnail: (cacheKey: string, id: number) => void;
  setFocusedId: Dispatch<SetStateAction<number | null>>;
  setExpandedBatches: Dispatch<SetStateAction<Set<string>>>;
  handleRestore: (id: number, format?: string) => Promise<void>;
  handleArchiveImages: (id: number) => Promise<void>;
  handleAuthorizeSource: (id: number) => Promise<void>;
  handleImportSourcePacket: (id: number) => Promise<void>;
  handleRevokeSource: () => Promise<void>;
  canRevokeSource: boolean;
  handleRestoreBatch: (batchId: string) => Promise<void>;
  handleDelete: (id: number) => Promise<void>;
  handleFavoriteChange: (entry: HistoryEntry) => Promise<void>;
  handleFavoriteProtected: () => void;
  collection: HistoryCollection;
}

export interface HistoryMainContentProps {
  shortcutNumbers: ReadonlyMap<number, number>;
  shortcutsActive: boolean;
  t: Translate;
  themeAssetSlots?: Record<string, boolean> | null;
  loading: boolean;
  entries: HistoryEntry[];
  hasActiveFilters: boolean;
  calendarNow: Date;
  expandedBatches: Set<string>;
  newIds: Set<number>;
  selectedId: number | null;
  focusedId: number | null;
  pageAnimationRevision: number;
  thumbnails: Map<string, ThumbnailData>;
  loadThumbnail: (cacheKey: string, id: number) => void;
  setFocusedId: Dispatch<SetStateAction<number | null>>;
  setExpandedBatches: Dispatch<SetStateAction<Set<string>>>;
  handleRestore: (id: number, format?: string) => Promise<void>;
  handleArchiveImages: (id: number) => Promise<void>;
  handleAuthorizeSource: (id: number) => Promise<void>;
  handleImportSourcePacket: (id: number) => Promise<void>;
  handleRevokeSource: () => Promise<void>;
  canRevokeSource: boolean;
  handleRestoreBatch: (batchId: string) => Promise<void>;
  handleDelete: (id: number) => Promise<void>;
  handleFavoriteChange: (entry: HistoryEntry) => Promise<void>;
  handleFavoriteProtected: () => void;
  collection: HistoryCollection;
  isFavoritesCollection: boolean;
}

export interface HistoryFooterProps {
  t: Translate;
  entriesLength: number;
  hasPrev: boolean;
  hasNext: boolean;
  page: number;
  totalEntries: number | null;
  totalPages: number;
  turnPage: (direction: number) => void;
  pageBusy: boolean;
  progressBarEnabled: boolean;
  fileProgress: FileProgress | null;
  handleCancelFileBatch: (batchId: string) => Promise<void>;
  showClearConfirm: boolean;
  clearing: boolean;
  setShowClearConfirm: Dispatch<SetStateAction<boolean>>;
  handleClearHistory: () => Promise<void>;
  isFavoritesCollection: boolean;
  historyNotice: HistoryNotice | null;
  clearHistoryNotice: () => void;
  retryHistory: () => void;
  retryRestore?: () => void;
}
