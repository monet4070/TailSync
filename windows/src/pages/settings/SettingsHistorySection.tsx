import type { SettingsHistorySectionProps } from "./SettingsSectionTypes";

export function SettingsHistorySection({
  t,
  historyLimitDraft,
  setHistoryLimitDraft,
  commitHistoryLimit,
}: SettingsHistorySectionProps) {
  return (
    <section className="setting-group">
      <div className="setting-group-header">
        <h3>{t("settings.history")}</h3>
        <p>{t("settings.historyDescription")}</p>
      </div>

      <div className="setting-row">
        <div className="setting-row-info">
          <span>{t("settings.historyLimit")}</span>
          <small>
            {t("settings.historyLimitDescriptionPrefix")} {historyLimitDraft}{" "}
            {t("settings.historyLimitDescriptionSuffix")}
          </small>
        </div>
        <input
          type="range"
          min={10}
          max={500}
          value={historyLimitDraft}
          aria-label={t("settings.historyLimit")}
          onChange={(event) => setHistoryLimitDraft(Number(event.target.value))}
          onPointerUp={() => void commitHistoryLimit()}
          onBlur={() => void commitHistoryLimit()}
          onKeyUp={(event) => {
            if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
              void commitHistoryLimit();
            }
          }}
        />
        <span className="range-value">{historyLimitDraft}</span>
      </div>
    </section>
  );
}
