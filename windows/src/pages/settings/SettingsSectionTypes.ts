import type { Dispatch, RefObject, SetStateAction } from "react";
import type {
  PairingStatus,
  PeerDevice,
  PeerRoute,
  PeersResponse,
  StorageMigrationResult,
  StorageStatus,
  ThemeV2Descriptor,
  UpdateStatus,
} from "../../tailsyncClient";
import type { ConnectionTestState } from "../../hooks/useConnectionTests";
import type { useShortcutRecorder } from "../../hooks/useShortcutRecorder";
import type { ThemePreference } from "../../hooks/useTheme";
import type { UpdatePhase } from "../../hooks/useUpdater";
import type { SettingsData } from "../../types/settings.generated";
import type { PendingThemePackage } from "../../utils/themePackageWorkflow";

export type Translate = (key: string) => string;

export type ShortcutRecorder = ReturnType<typeof useShortcutRecorder>;

export interface SettingsConnectionsSectionProps {
  settings: SettingsData;
  t: Translate;
  devices: PeersResponse | null;
  devicesLoading: boolean;
  devicesError: string;
  pairingStatus: PairingStatus | null;
  pairingBusy: boolean;
  connectionTests: Record<string, ConnectionTestState>;
  refreshDevices: () => Promise<void>;
  handleConnectionMode: (mode: SettingsData["connection_mode"]) => Promise<void>;
  closePairing: () => Promise<void>;
  handleEnablePairing: () => Promise<void>;
  handleTestConnection: (peer: PeerDevice, route: PeerRoute) => Promise<void>;
  handlePeerToggle: (peer: PeerDevice, enabled: boolean) => Promise<void>;
  handleForget: (peer: PeerDevice) => Promise<void>;
  openPairing: (peer: PeerDevice) => Promise<void>;
}

export interface SettingsGeneralSectionProps {
  settings: SettingsData;
  t: Translate;
  syncShortcutRecorder: ShortcutRecorder;
  historyShortcutRecorder: ShortcutRecorder;
  setGlobalSync: (enabled: boolean) => Promise<void>;
  update: (patch: Partial<SettingsData>) => Promise<boolean>;
}

export interface SettingsHistorySectionProps {
  t: Translate;
  historyLimitDraft: number;
  setHistoryLimitDraft: Dispatch<SetStateAction<number>>;
  commitHistoryLimit: () => Promise<void>;
}

export interface SettingsStorageSectionProps {
  settings: SettingsData;
  t: Translate;
  storageStatus: StorageStatus | null;
  storageBusy: boolean;
  storageQuotaDraft: string;
  oldStorage: StorageMigrationResult | null;
  setStorageQuotaDraft: Dispatch<SetStateAction<string>>;
  setOldStorage: Dispatch<SetStateAction<StorageMigrationResult | null>>;
  changeStorage: () => Promise<void>;
  commitStorageQuota: () => Promise<void>;
  handleDeleteOldStorage: () => Promise<void>;
}

export interface SettingsAppearanceSectionProps {
  settings: SettingsData;
  t: Translate;
  locale: string;
  themePreference: ThemePreference;
  v2Themes: ThemeV2Descriptor[];
  v2Active: string;
  setLocale: (locale: string) => void;
  changeThemePreference: (value: ThemePreference) => Promise<void>;
  selectV2Theme: (id: string) => Promise<void>;
  handleImportTheme: () => void;
  handleUpdateTheme: (entry: ThemeV2Descriptor) => void;
  rollbackV2Theme: (entry: ThemeV2Descriptor) => Promise<void>;
  deleteV2Theme: (entry: ThemeV2Descriptor) => Promise<void>;
  changeLanguage: (language: SettingsData["language"]) => Promise<void>;
}

export interface SettingsUpdateSectionProps {
  t: Translate;
  updateStatus: UpdateStatus | null;
  updatePhase: UpdatePhase;
  availableUpdate: { version: string } | null;
  updateMessage: string;
  updateBusy: boolean;
  handleCheckForUpdate: () => Promise<void>;
  handleInstallUpdate: () => Promise<void>;
}

export interface PairingDialogProps {
  t: Translate;
  pairingOpen: boolean;
  pairingStatus: PairingStatus | null;
  pairingTarget: PeerDevice | null;
  pairingError: string;
  pairingBusy: boolean;
  pairDialogRef: RefObject<HTMLDivElement | null>;
  closePairing: () => Promise<void>;
  handlePair: () => Promise<void>;
}

export interface ThemeImportDialogProps {
  t: Translate;
  pendingThemeImport: PendingThemePackage | null;
  setPendingThemeImport: Dispatch<SetStateAction<PendingThemePackage | null>>;
  confirmThemeImport: () => Promise<void>;
}
