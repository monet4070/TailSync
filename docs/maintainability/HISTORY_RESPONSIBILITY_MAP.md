# TailSync Maintainability Refactor — HISTORY RESPONSIBILITY MAP (T220/T249)

> 生成时间：2026-08-15（T243/T244 typed client 迁移后更新）
> 依据：T000 全量阅读（1374 行时点）+ T243/T244 迁移后复核（当前 1319 行）。
> 只读交付。本图是 R007（T250+ 增量提取）的唯一事实来源。

---

## 1. UI 分区（FACT，行号=当前文件，迁移后偏移）

| Section | 内容 |
|---|---|
| 标题栏 | 品牌、ThemeLogo、`getCurrentWindow().hide()` |
| 工具栏 | 搜索框（keywordDraft 防抖 250ms）、清空按钮、分类/日期 FilterDropdown（177-276 时点） |
| 自定义日期范围 | selectedDateFilter==="custom" 时 |
| 结果计数 / 迁移警告横幅 | unresolved_count>0 |
| 骨架 / 空态 | loading 且无条目 / 无匹配 |
| 历史列表（日期分组） | 分组 IIFE + 批次头（copy-all/incomplete/show-more）+ 条目 article |
| 分页 | prev/next + 页码（historyPagination.ts 提供数学） |
| 文件进度条 | progressBarEnabled && fileProgress.active |
| Toast / 清空确认对话框 | 互斥三元 / alertdialog |

子组件（文件内）：FilterDropdown、ThumbnailCanvas（RGBA canvas 渲染）、LazyThumbnail（IntersectionObserver）。

## 2. 状态（FACT，T000 实测 24 useState + 12 useRef + 3 useMemo；迁移后未变）

- 24 useState：entries、totalEntries、hasMoreEntries、capabilities、migrationDiagnostics、thumbnails（Map）、keywordDraft、keyword、selectedCategory、selectedDateFilter、customStartDate、customEndDate、calendarNow、page、selectedId、loading、newIds、expandedBatches、showClearConfirm、clearing、actionError、syncWarning、fileProgress（FileProgress|null，T243 后）、progressBarEnabled。
- 12 useRef：thumbnailIds、calendarContextKey、restoreFeedbackTimer、actionErrorTimer、syncWarningTimer、newGlowTimers、lastVersion、prevIds、lastQueryKey、historyRequests（LatestRequest）+ 子组件 refs。
- 3 useMemo：categoryOptions、dateOptions、activeDateBounds。

## 3. Effects 与轮询（FACT）

- 10 useEffect（页面级 7 + 子组件 3：FilterDropdown 外点/Escape、ThumbnailCanvas 重绘、LazyThumbnail IntersectionObserver）。
- 页面级：unmount 计时器清理、日历时钟（60s + focus/visibility）、初始 capabilities+diagnostics 加载、keyword 防抖（250ms）、filter 变更重置 page、loadHistory 身份依赖重载。
- **useVisiblePolling ×2**：loadSettings @5000ms；版本/同步警告/进度 @800ms（版本变化触发 loadHistory）。

## 4. 命令面（T243/T244 后：全部经 tailsyncClient，零直接 invoke）

- 读侧：getSettings（progress_bar_enabled）、getImageData(id)、getMigrationDiagnostics、getHistoryCapabilities、getHistoryPage(query)、getVersion、getSyncWarning、getFileProgress。
- 写侧：restoreEntry(id)、deleteEntry(id)、clearHistory、restoreFileBatch(batchId)、setHistoryPinned(id,pinned)、cancelFileBatch(batchId)。
- 事件订阅：无 listen（纯轮询协议）。

## 5. 职责归属（FACT，T000 实测 + 迁移后复核）

| 职责 | 位置 | 说明 |
|---|---|---|
| pagination | `utils/historyPagination.ts`（已抽离） | HISTORY_PAGE_SIZE/historyPageCount/normalizeHistoryPage |
| async 过期 | `utils/asyncControl.ts` LatestRequest（已抽离） | historyRequests ref 使用 |
| polling | `hooks/useVisiblePolling.ts`（已抽离） | 2 处使用 |
| **query** | 页内 loadHistory（L679-758 时点，~80 行 7 依赖内联回调） | query 组装 + 分页 + new-glow diff |
| **filter** | 页内（dateBounds/本地日期 helper/options useMemo） | 分类选项按 capabilities 过滤 |
| **preview** | 页内（loadThumbnail L630-657 时点 + ThumbnailCanvas/LazyThumbnail） | 缩略图 LRU 上限 MAX_CACHED_THUMBNAILS |
| **render** | 页内（分组 IIFE L1080-1261 时点 + 批次折叠算术） | 最大 JSX 块 |
| **cache** | 页内（thumbnails Map + thumbnailIds + lastVersion/lastQueryKey refs） | 缓存键与失效 |

## 6. 复杂度热点（T250+ 拆分输入）

1. loadHistory 80 行内联回调（查询/分页/缓存/glow 全包）。
2. 分组 IIFE（批次折叠/展开动画算术内嵌 JSX）。
3. 日期解析/校验 helper 三件（localDateFromInput/dateBounds/dateInputValue）与日期语义重叠。
4. 4× 重复 toast 超时模式（restore/error/syncWarning）。
5. `document.querySelector(".history-list")?.scrollTo` ×2（分页后滚顶）。
6. 测试内 mock invoke switch 块重复 4×（K 系列已记录）。

## 7. T250+ 提取建议（按隔离度排序，BEHAVIOR_CHANGE_ALLOWED=FALSE）

1. **日期/筛选纯逻辑 → utils/historyFilters.ts**（localDateFromInput/dateBounds/dateInputValue/options 构建——纯函数，直接单测）。
2. **toast 计时 → hooks/useTransientToasts.ts**（restore/error/syncWarning 三套计时统一）。
3. **loadHistory 拆分**：query 组装 → `utils/historyQuery.ts`（纯函数 buildHistoryQuery(filters, page, pageSize)），loadHistory 内联体瘦身。
4. **分组渲染 → components/HistoryGroupList.tsx**（分组 IIFE + 批次折叠迁出，props: entries/newIds/expandedBatches/callbacks）。
5. **缩略图缓存 → hooks/useThumbnailCache.ts**（Map + LRU 上限 + loadThumbnail）。

每个提取：现有 vitest（5 用例）+ build/lint/test 全绿；纯函数部分可直接新增单测。
