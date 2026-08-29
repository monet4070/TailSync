import {
  DEFAULT_HISTORY_SHORTCUT,
  DEFAULT_SYNC_SHORTCUT,
} from "../../hooks/useShortcutRecorder";
import { ShortcutSettingRow } from "./SettingsShortcutControls";
import type { SettingsGeneralSectionProps } from "./SettingsSectionTypes";

export function SettingsGeneralSection({
  settings,
  t,
  syncShortcutRecorder,
  historyShortcutRecorder,
  setGlobalSync,
  update,
}: SettingsGeneralSectionProps) {
  return (
    <section className="setting-group">
      <div className="setting-group-header">
        <h3>{t("settings.general")}</h3>
        <p>{t("settings.generalDescription")}</p>
      </div>

      <div
        className="setting-row setting-row--toggle"
        onClick={() => void setGlobalSync(!settings.sync_enabled)}
      >
        <div className="setting-row-info">
          <span>{t("settings.syncEnabled")}</span>
          <small>{t("settings.syncEnabledDescription")}</small>
        </div>
        <label className="toggle" onClick={(event) => event.stopPropagation()}>
          <input
            type="checkbox"
            checked={settings.sync_enabled}
            onChange={(event) => void setGlobalSync(event.target.checked)}
          />
          <div className="toggle-track" />
        </label>
      </div>

      <ShortcutSettingRow
        recorder={syncShortcutRecorder}
        currentShortcut={settings.sync_shortcut}
        defaultShortcut={DEFAULT_SYNC_SHORTCUT}
        title={t("settings.syncShortcut")}
        description={t("settings.syncShortcutDescription")}
        recordLabel={t("settings.shortcutRecord")}
        t={t}
        disabled={historyShortcutRecorder.shortcutRecording}
      />

      <ShortcutSettingRow
        recorder={historyShortcutRecorder}
        currentShortcut={settings.history_shortcut}
        defaultShortcut={DEFAULT_HISTORY_SHORTCUT}
        title={t("settings.historyShortcut")}
        description={t("settings.historyShortcutDescription")}
        recordLabel={t("settings.historyShortcutRecord")}
        t={t}
        disabled={syncShortcutRecorder.shortcutRecording}
      />

      <div
        className="setting-row setting-row--toggle"
        onClick={() => void update({
          notifications_enabled: !settings.notifications_enabled,
        })}
      >
        <div className="setting-row-info">
          <span>{t("settings.notifications")}</span>
          <small>{t("settings.notificationsDescription")}</small>
        </div>
        <label className="toggle" onClick={(event) => event.stopPropagation()}>
          <input
            type="checkbox"
            checked={settings.notifications_enabled}
            onChange={(event) => void update({ notifications_enabled: event.target.checked })}
          />
          <div className="toggle-track" />
        </label>
      </div>

      <div
        className="setting-row setting-row--toggle"
        onClick={() => void update({
          progress_bar_enabled: !settings.progress_bar_enabled,
        })}
      >
        <div className="setting-row-info">
          <span>{t("settings.progressBar")}</span>
          <small>{t("settings.progressDescription")}</small>
        </div>
        <label className="toggle" onClick={(event) => event.stopPropagation()}>
          <input
            type="checkbox"
            checked={settings.progress_bar_enabled}
            onChange={(event) => void update({ progress_bar_enabled: event.target.checked })}
          />
          <div className="toggle-track" />
        </label>
      </div>
    </section>
  );
}
