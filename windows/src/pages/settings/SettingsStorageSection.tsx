import { FolderOpen, HardDrive } from "lucide-react";
import { formatStorageSize } from "./SettingsFormatters";
import type { SettingsStorageSectionProps } from "./SettingsSectionTypes";

export function SettingsStorageSection({
  settings,
  t,
  storageStatus,
  storageBusy,
  storageQuotaDraft,
  oldStorage,
  setStorageQuotaDraft,
  setOldStorage,
  changeStorage,
  commitStorageQuota,
  handleDeleteOldStorage,
}: SettingsStorageSectionProps) {
  return (
    <section className="setting-group">
      <div className="setting-group-header">
        <h3>{t("settings.storage")}</h3>
        <p>{t("settings.storageDescription")}</p>
      </div>
      <div className="setting-row storage-row">
        <HardDrive size={17} strokeWidth={1.7} aria-hidden="true" />
        <div className="setting-row-info storage-location">
          <span title={storageStatus?.root}>{storageStatus?.root ?? settings.storage_root ?? ""}</span>
          <small>
            {storageStatus?.available === false
              ? storageStatus.error
              : `${formatStorageSize(storageStatus?.used_bytes ?? 0)} / ${formatStorageSize(settings.storage_quota_bytes)}`}
          </small>
        </div>
        <button className="storage-change" type="button" onClick={() => void changeStorage()} disabled={storageBusy}>
          <FolderOpen size={15} strokeWidth={1.8} aria-hidden="true" />
          <span>{storageBusy ? t("settings.storageMoving") : t("settings.storageChange")}</span>
        </button>
      </div>
      <div className="setting-row storage-quota-row">
        <div className="setting-row-info">
          <span>{t("settings.storageQuota")}</span>
        </div>
        <input
          className="storage-quota-input"
          type="text"
          inputMode="numeric"
          pattern="[0-9]*"
          value={storageQuotaDraft}
          onChange={(event) => {
            setStorageQuotaDraft(event.target.value.replace(/\D/g, "").slice(0, 5));
          }}
          onBlur={() => void commitStorageQuota()}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              event.currentTarget.blur();
            }
          }}
          aria-label={t("settings.storageQuota")}
        />
        <span className="storage-quota-unit">GiB</span>
      </div>
      {oldStorage && oldStorage.old_root !== oldStorage.new_root && (
        <div className="old-storage-row">
          <span>{t("settings.storageOldData")} ({formatStorageSize(oldStorage.old_size_bytes)})</span>
          <div>
            <button type="button" onClick={() => void handleDeleteOldStorage()}>{t("settings.storageDeleteOld")}</button>
            <button type="button" onClick={() => setOldStorage(null)}>{t("settings.storageKeepOld")}</button>
          </div>
        </div>
      )}
    </section>
  );
}
