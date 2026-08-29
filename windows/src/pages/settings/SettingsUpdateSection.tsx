import { RefreshCw } from "lucide-react";
import type { SettingsUpdateSectionProps } from "./SettingsSectionTypes";

export function SettingsUpdateSection({
  t,
  updateStatus,
  updatePhase,
  availableUpdate,
  updateMessage,
  updateBusy,
  handleCheckForUpdate,
  handleInstallUpdate,
}: SettingsUpdateSectionProps) {
  return (
    <section className="setting-group update-group">
      <div className="setting-group-header">
        <h3>{t("settings.updates")}</h3>
        <p>{t("settings.updatesDescription")}</p>
      </div>
      <div className="setting-row update-row">
        <div className="setting-row-info update-version">
          <span>TailSync {updateStatus?.current_version ?? "-"}</span>
          <small className={updatePhase === "error" ? "update-status error" : "update-status"}>
            {updateMessage}
          </small>
        </div>
        <button
          className="update-action"
          type="button"
          onClick={() => void (availableUpdate ? handleInstallUpdate() : handleCheckForUpdate())}
          disabled={
            updateBusy
            || updatePhase === "loading"
            || updatePhase === "disabled"
            || updatePhase === "installed"
          }
        >
          <RefreshCw
            size={15}
            strokeWidth={1.8}
            className={updateBusy ? "spin" : undefined}
            aria-hidden="true"
          />
          <span>{availableUpdate ? t("settings.updateInstall") : t("settings.updateCheck")}</span>
        </button>
      </div>
    </section>
  );
}
