import {
  Check,
  ChevronDown,
  Monitor,
  Moon,
  RefreshCw,
  RotateCcw,
  Sun,
  Trash2,
  Upload,
} from "lucide-react";
import {
  palettePreviewClass,
  palettePreviewStyle,
  palettePreviewTitle,
} from "./SettingsThemePreview";
import type { SettingsAppearanceSectionProps } from "./SettingsSectionTypes";

export function SettingsAppearanceSection({
  settings,
  t,
  locale,
  themePreference,
  v2Themes,
  v2Active,
  setLocale,
  changeThemePreference,
  selectV2Theme,
  handleImportTheme,
  handleUpdateTheme,
  rollbackV2Theme,
  deleteV2Theme,
  changeLanguage,
}: SettingsAppearanceSectionProps) {
  return (
    <section className="setting-group">
      <div className="setting-group-header">
        <h3>{t("settings.appearance")}</h3>
        <p>{t("settings.appearanceDescription")}</p>
      </div>

      <div className="setting-row">
        <div className="setting-row-info">
          <span>{t("settings.colorMode")}</span>
        </div>
        <div className="theme-cards" role="group" aria-label={t("settings.colorMode")}>
          <button
            type="button"
            className={`theme-card${themePreference === "system" ? " active" : ""}`}
            onClick={() => void changeThemePreference("system")}
            aria-pressed={themePreference === "system"}
          >
            <Monitor className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
            <span>{t("settings.themeSystem")}</span>
          </button>
          <button
            type="button"
            className={`theme-card${themePreference === "light" ? " active" : ""}`}
            onClick={() => void changeThemePreference("light")}
            aria-pressed={themePreference === "light"}
          >
            <Sun className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
            <span>{t("settings.themeLight")}</span>
          </button>
          <button
            type="button"
            className={`theme-card${themePreference === "dark" ? " active" : ""}`}
            onClick={() => void changeThemePreference("dark")}
            aria-pressed={themePreference === "dark"}
          >
            <Moon className="theme-mode-icon" size={16} strokeWidth={1.6} aria-hidden="true" />
            <span>{t("settings.themeDark")}</span>
          </button>
        </div>
      </div>

      <div className="setting-row palette-setting-row">
        <div className="setting-row-info">
          <span>{t("settings.colorTheme")}</span>
          <small>{t("settings.colorThemeDescription")}</small>
        </div>
        <div className="theme-cards palette-cards" role="group" aria-label={t("settings.colorTheme")}>
          {v2Themes.map((entry) => {
            const active = entry.id === v2Active;
            const label = entry.name[locale] ?? entry.name.en ?? entry.id;
            return (
              <button
                type="button"
                key={entry.id}
                aria-disabled={entry.status !== "valid"}
                className={`theme-card palette-card ${palettePreviewClass(entry.id)}${active ? " active" : ""}${entry.status !== "valid" ? " is-invalid" : ""}`}
                style={palettePreviewStyle(entry)}
                onClick={() => { if (entry.status === "valid") void selectV2Theme(entry.id); }}
                aria-pressed={active}
                title={entry.diagnostics.map((diagnostic) => diagnostic.message).join("\n") || label}
              >
                <div
                  className={`palette-card-preview ${palettePreviewClass(entry.id)}${entry.status !== "valid" ? " invalid" : ""}`}
                  aria-hidden="true"
                >
                  <span className="palette-preview-rail" />
                  <span className="palette-preview-title">{entry.status === "valid" ? palettePreviewTitle(entry.id) : "!"}</span>
                  <span className="palette-preview-swatch swatch-accent" />
                  <span className="palette-preview-swatch swatch-secondary" />
                  <span className="palette-preview-swatch swatch-border" />
                  <span className="palette-preview-rule" />
                  <span className="palette-preview-control control-input"><i /></span>
                  <span className="palette-preview-control control-action"><i /></span>
                </div>
                <span className="palette-card-label">
                  <strong>{label}</strong>
                  <small>{entry.source === "builtin" ? t("settings.themePackageBuiltIn") : entry.version}</small>
                </span>
                {active && <Check className="palette-card-check" size={13} strokeWidth={2} aria-hidden="true" />}
                {(entry.source === "custom" || entry.status === "invalid") && (
                  <span className="theme-card-tools">
                    {entry.source === "custom" && entry.status === "valid" && (
                      <>
                        <span
                          role="button"
                          tabIndex={0}
                          className="theme-card-tool"
                          onClick={(event) => { event.stopPropagation(); handleUpdateTheme(entry); }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              event.stopPropagation();
                              handleUpdateTheme(entry);
                            }
                          }}
                          title={t("settings.customThemeUpdate")}
                          aria-label={`${t("settings.customThemeUpdate")} ${label}`}
                        >
                          <RefreshCw size={12} strokeWidth={1.8} aria-hidden="true" />
                        </span>
                        <span
                          role="button"
                          tabIndex={0}
                          className="theme-card-tool"
                          onClick={(event) => { event.stopPropagation(); void rollbackV2Theme(entry); }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              event.stopPropagation();
                              void rollbackV2Theme(entry);
                            }
                          }}
                          title={t("settings.customThemeRollback")}
                          aria-label={`${t("settings.customThemeRollback")} ${label}`}
                        >
                          <RotateCcw size={12} strokeWidth={1.8} aria-hidden="true" />
                        </span>
                      </>
                    )}
                    <span
                      role="button"
                      tabIndex={0}
                      className="theme-card-tool custom-theme-delete"
                      onClick={(event) => { event.stopPropagation(); void deleteV2Theme(entry); }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          event.stopPropagation();
                          void deleteV2Theme(entry);
                        }
                      }}
                      title={t("settings.customThemeDelete")}
                      aria-label={`${t("settings.customThemeDelete")} ${label}`}
                    >
                      <Trash2 size={12} strokeWidth={1.8} aria-hidden="true" />
                    </span>
                  </span>
                )}
              </button>
            );
          })}
        </div>
        <div className="custom-themes-actions">
          <button type="button" className="custom-theme-action" onClick={handleImportTheme}>
            <Upload size={13} strokeWidth={1.8} aria-hidden="true" />
            {t("settings.customThemeImport")}
          </button>
        </div>
      </div>

      <div className="setting-row">
        <div className="setting-row-info">
          <span>{t("settings.language")}</span>
        </div>
        <div className="select-shell">
          <select
            value={settings.language}
            onChange={(event) => {
              const language = event.target.value as typeof settings.language;
              setLocale(language);
              void changeLanguage(language);
            }}
          >
            <option value="zh-CN">简体中文</option>
            <option value="en">English</option>
          </select>
          <ChevronDown size={14} strokeWidth={1.7} aria-hidden="true" />
        </div>
      </div>
    </section>
  );
}
