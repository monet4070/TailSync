import { Pin, Search, Trash2, TriangleAlert, X } from "lucide-react";
import { ThemeLogo } from "../../ThemeLogo";
import { FilterDropdown } from "./HistoryFilterDropdown";
import type { HistoryHeaderProps } from "./HistoryViewTypes";

export function HistoryHeader({
  t,
  windowAlwaysOnTop,
  windowAlwaysOnTopPending,
  toggleWindowAlwaysOnTop,
  closeHistory,
  keywordDraft,
  setKeywordDraft,
  totalEntries,
  setShowClearConfirm,
  selectedCategory,
  setSelectedCategory,
  categoryOptions,
  selectedDateFilter,
  dateOptions,
  handleDateFilterChange,
  dateRangeFilterEnabled,
  customStartDate,
  customEndDate,
  setCustomStartDate,
  setCustomEndDate,
  activeDateBounds,
  hasActiveFilters,
  migrationDiagnostics,
}: HistoryHeaderProps) {
  return (
    <>
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-brand">
          <ThemeLogo />
          <span className="titlebar-text">TailSync</span>
          <span className="titlebar-badge">v2</span>
        </div>
        <div className="titlebar-actions">
          <button
            className={`titlebar-btn titlebar-pin${windowAlwaysOnTop ? " active" : ""}`}
            type="button"
            onClick={() => void toggleWindowAlwaysOnTop()}
            disabled={windowAlwaysOnTopPending}
            title={t(windowAlwaysOnTop ? "history.unpinWindow" : "history.pinWindow")}
            aria-label={t(windowAlwaysOnTop ? "history.unpinWindow" : "history.pinWindow")}
            aria-pressed={windowAlwaysOnTop}
          >
            <Pin size={14} fill={windowAlwaysOnTop ? "currentColor" : "none"} aria-hidden="true" />
          </button>
          <button
            className="titlebar-close"
            type="button"
            onClick={() => void closeHistory()}
            title={t("history.close")}
            aria-label={t("history.close")}
          >
            <X size={15} strokeWidth={1.8} aria-hidden="true" />
          </button>
        </div>
      </div>

      <div className="history-toolbar">
        <div className="search-bar">
          <span className="search-icon">
            <Search size={18} strokeWidth={1.7} aria-hidden="true" />
          </span>
          <input
            type="text"
            placeholder={t("history.searchPlaceholder")}
            value={keywordDraft}
            onChange={(event) => setKeywordDraft(event.target.value)}
            autoFocus
          />
          <button
            className="clear-history-btn"
            type="button"
            disabled={totalEntries === 0}
            onClick={() => setShowClearConfirm(true)}
            title={t("history.clearAll")}
            aria-label={t("history.clearAll")}
          >
            <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
          </button>
        </div>

        <div className="history-filter-bar">
          <FilterDropdown
            value={selectedCategory}
            options={categoryOptions}
            label={t("history.categoryFilter")}
            testId="category-filter"
            onChange={(value) => setSelectedCategory(value as "all" | typeof selectedCategory)}
          />
          {dateRangeFilterEnabled && (
            <FilterDropdown
              value={selectedDateFilter}
              options={dateOptions}
              label={t("history.dateFilter")}
              testId="date-filter"
              onChange={handleDateFilterChange}
            />
          )}
        </div>
      </div>

      {dateRangeFilterEnabled && selectedDateFilter === "custom" && (
        <div className="custom-date-range" data-testid="custom-date-range">
          <label>
            <span>{t("history.date.start")}</span>
            <input
              type="date"
              value={customStartDate}
              max={customEndDate || undefined}
              aria-invalid={!activeDateBounds.valid}
              aria-describedby={!activeDateBounds.valid ? "custom-date-error" : undefined}
              onChange={(event) => setCustomStartDate(event.target.value)}
            />
          </label>
          <span className="date-range-separator" aria-hidden="true">–</span>
          <label>
            <span>{t("history.date.end")}</span>
            <input
              type="date"
              value={customEndDate}
              min={customStartDate || undefined}
              aria-invalid={!activeDateBounds.valid}
              aria-describedby={!activeDateBounds.valid ? "custom-date-error" : undefined}
              onChange={(event) => setCustomEndDate(event.target.value)}
            />
          </label>
          {!activeDateBounds.valid && (
            <span className="date-range-error" id="custom-date-error" role="status">
              {t("history.date.invalid")}
            </span>
          )}
        </div>
      )}

      {hasActiveFilters && totalEntries !== null && totalEntries > 0 && (
        <div className="search-results-count">
          {totalEntries} {t(totalEntries === 1 ? "history.result" : "history.results")}
        </div>
      )}

      {migrationDiagnostics && migrationDiagnostics.unresolved_count > 0 && (
        <div className="migration-warning" role="status">
          <TriangleAlert size={15} strokeWidth={1.8} aria-hidden="true" />
          <span>
            {t("history.migrationWarningPrefix")} {migrationDiagnostics.unresolved_count}{" "}
            {t("history.migrationWarningSuffix")}
          </span>
        </div>
      )}
    </>
  );
}
