import {
  Code2,
  Database,
  File,
  Folder,
  Globe2,
  Image as ImageIcon,
  Terminal,
  Type,
} from "lucide-react";
import type { HistoryCategory } from "../../tailsyncClient";
import { HISTORY_PAGE_SIZE } from "../../utils/historyPagination";

export const PAGE_SIZE = HISTORY_PAGE_SIZE;
export const RUNTIME_WAIT_MS = 2_500;
export const SETTINGS_POLL_MS = 5000;
export const SEARCH_DEBOUNCE_MS = 250;
export const MAX_CACHED_THUMBNAILS = PAGE_SIZE;
export const NEW_GLOW_DURATION_MS = 3000;
export const RESTORE_FEEDBACK_DURATION_MS = 1500;
export const COLLAPSED_BATCH_FILE_LIMIT = 2;
export const HISTORY_ALWAYS_ON_TOP_STORAGE_KEY = "tailsync-history-always-on-top";
// Animate only the rows that can plausibly be visible in the history window.
// Animating an entire 50-row page creates dozens of WebView compositor layers.
export const MAX_PAGE_ENTER_ITEMS = 12;
export const HISTORY_CATEGORIES: HistoryCategory[] = [
  "text",
  "website",
  "code",
  "command",
  "structured_data",
  "path",
  "image",
  "file",
];
export const CATEGORY_FILTERS: Array<"all" | HistoryCategory> = [
  "all",
  ...HISTORY_CATEGORIES,
];

export const CATEGORY_ICONS = {
  text: Type,
  website: Globe2,
  code: Code2,
  command: Terminal,
  structured_data: Database,
  path: Folder,
  image: ImageIcon,
  file: File,
} satisfies Record<HistoryCategory, typeof Type>;
