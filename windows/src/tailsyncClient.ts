// Typed Tauri command boundary.
//
// The Windows UI reaches the Rust command layer only through this module
// (R008). Feature sections land here incrementally (T241+); JSON command
// names and payload shapes are the wire contract and must not change.

import { invoke } from "@tauri-apps/api/core";
import type { PreviewResponseInput } from "./utils/historyPreview";
import type { SettingsData } from "./types/settings.generated";

// ---------------------------------------------------------------------------
// Updater (T241 pilot)
// ---------------------------------------------------------------------------

export interface UpdateStatus {
  current_version: string;
  updates_enabled: boolean;
}

export interface UpdateInfo {
  current_version: string;
  version: string;
  notes?: string | null;
  published_at?: string | null;
}

export function getUpdateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("get_update_status");
}

export function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

export function installUpdate(): Promise<boolean> {
  return invoke<boolean>("install_update");
}

// ---------------------------------------------------------------------------
// Storage (T242)
// ---------------------------------------------------------------------------

export interface StorageStatus {
  root: string;
  used_bytes: number;
  quota_bytes: number;
  available: boolean;
  error?: string | null;
}

export interface StorageMigrationResult {
  new_root: string;
  old_root: string;
  old_size_bytes: number;
}

export function getStorageStatus(): Promise<StorageStatus> {
  return invoke<StorageStatus>("get_storage_status");
}

export function changeStorageLocation(parent: string): Promise<StorageMigrationResult> {
  return invoke<StorageMigrationResult>("change_storage_location", { parent });
}

export function deleteOldStorage(path: string): Promise<void> {
  return invoke<void>("delete_old_storage", { path });
}

// ---------------------------------------------------------------------------
// Settings — read (shared by both pages; write-side migrates later)
// ---------------------------------------------------------------------------

export function getSettings(): Promise<SettingsData> {
  return invoke<SettingsData>("get_settings");
}

// ---------------------------------------------------------------------------
// History — read side (T243)
// ---------------------------------------------------------------------------

export type HistoryCategory =
  | "text"
  | "website"
  | "code"
  | "command"
  | "structured_data"
  | "path"
  | "image"
  | "file";

export interface HistoryEntry {
  id: number;
  timestamp: string;
  type: "text" | "image" | "file";
  description: string;
  data_hash: string;
  size_bytes: number;
  source_peer: string;
  category?: HistoryCategory;
  categories?: HistoryCategory[];
  category_confidence?: number;
  classifier_version?: number;
  pinned?: boolean;
  batch_id?: string | null;
  batch_index?: number | null;
  batch_total?: number | null;
  batch_count?: number | null;
  batch_status?: "complete" | "incomplete";
}

export interface ImageThumbnail {
  id: number;
  thumbnail_b64: string;
  thumbnail_width: number;
  thumbnail_height: number;
}

export interface HistoryPageResult {
  entries: HistoryEntry[];
  total: number | null;
  has_more: boolean;
}

export interface HistoryCapabilities {
  classifier_version: number;
  categories: HistoryCategory[];
  multiple_labels: boolean;
  date_range_filter: boolean;
}

export interface MigrationDiagnostics {
  unresolved_count: number;
}

export interface SyncWarning {
  kind: "expired_event";
  peer: string;
  occurred_at_ms: number;
}

export interface FileProgress {
  batch_id: string;
  name: string;
  sent: number;
  total: number;
  active: boolean;
  direction: "sending" | "receiving";
  device: string;
  completed_files: number;
  total_files: number;
  speed_bytes_per_second: number;
  status: string;
  can_stop: boolean;
}

export type HistoryPageQuery = {
  keyword: string | null;
  category: HistoryCategory | null;
  startTime: string | null;
  endTime: string | null;
  limit: number;
  offset: number;
};

export function getImageData(id: number): Promise<ImageThumbnail> {
  return invoke<ImageThumbnail>("get_image_data", { id });
}

/**
 * Load a full history preview through Tauri's raw IPC response path.
 *
 * The returned ArrayBuffer uses the versioned TSPV envelope decoded by
 * `utils/historyPreview.ts`; keeping the payload binary avoids the memory and
 * CPU overhead of base64 for previews up to the shared 64 MiB limit.
 */
export function getPreview(id: number, batchId?: string | null): Promise<PreviewResponseInput> {
  return invoke<PreviewResponseInput>("get_preview", {
    id,
    ...(batchId ? { batchId } : {}),
  });
}

export interface PreviewWindowRequest {
  entryId: number;
  batchId?: string | null;
}

export interface PreviewWindowSnapshot {
  revision: number;
  entryId: number;
  batchId: string | null;
}

export function openPreviewWindow(
  entryId: number,
  batchId?: string | null,
): Promise<PreviewWindowSnapshot> {
  return invoke<PreviewWindowSnapshot>("open_preview_window", {
    request: { entryId, batchId: batchId ?? null },
  });
}

export function getPreviewWindowRequest(): Promise<PreviewWindowSnapshot | null> {
  return invoke<PreviewWindowSnapshot | null>("get_preview_window_request");
}

export function closePreviewWindow(): Promise<void> {
  return invoke<void>("close_preview_window");
}

export function syncPreviewWindowMinimized(minimized: boolean): Promise<void> {
  return invoke<void>("sync_preview_window_minimized", { minimized });
}

export function getMigrationDiagnostics(): Promise<MigrationDiagnostics> {
  return invoke<MigrationDiagnostics>("get_migration_diagnostics");
}

export function getHistoryCapabilities(): Promise<HistoryCapabilities> {
  return invoke<HistoryCapabilities>("get_history_capabilities");
}

export function getHistoryPage(query: HistoryPageQuery): Promise<HistoryPageResult> {
  return invoke<HistoryPageResult>("get_history_page", query);
}

export function getVersion(): Promise<{ version: number }> {
  return invoke<{ version: number }>("get_version");
}

export function getSyncWarning(): Promise<SyncWarning | null> {
  return invoke<SyncWarning | null>("get_sync_warning");
}

export function getFileProgress(): Promise<FileProgress> {
  return invoke<FileProgress>("get_file_progress");
}

// ---------------------------------------------------------------------------
// History — write side (T244)
// ---------------------------------------------------------------------------

export function restoreEntry(id: number): Promise<void> {
  return invoke<void>("restore_entry", { id });
}

export function deleteEntry(id: number): Promise<void> {
  return invoke<void>("delete_entry", { id });
}

export function clearHistory(): Promise<void> {
  return invoke<void>("clear_history");
}

export function restoreFileBatch(batchId: string): Promise<void> {
  return invoke<void>("restore_file_batch", { batchId });
}

export function setHistoryPinned(id: number, pinned: boolean): Promise<void> {
  return invoke<void>("set_history_pinned", { id, pinned });
}

export function cancelFileBatch(batchId: string): Promise<void> {
  return invoke<void>("cancel_file_batch", { batchId });
}

// ---------------------------------------------------------------------------
// Devices & pairing (T245)
// ---------------------------------------------------------------------------

export interface PeerRoute {
  interface: "lan" | "iroh" | "tailscale";
  address: string;
  status: "discovered" | "online" | "confirming" | "offline" | "connected";
  online: boolean;
  connected: boolean;
  latency_ms?: number | null;
  pairing_endpoint?: boolean;
  rtt_capable?: boolean;
}

export interface PeerDevice {
  hostname: string;
  tailscale_ip: string;
  address: string;
  online: boolean;
  enabled: boolean;
  connection_mode: "auto" | "lan" | "tailscale";
  trusted: boolean;
  fingerprint: string;
  current_interface?: "lan" | "iroh" | "tailscale";
  current_address?: string | null;
  status?: "discovered" | "online" | "confirming" | "offline" | "connected";
  protocol_error?: string | null;
  required_protocol_version?: number | null;
  routes?: PeerRoute[];
}

export interface PeersResponse {
  self: {
    hostname: string;
    tailscale_ip: string;
    connection_mode: "auto" | "lan_only" | "tailscale_only";
    public_key: string;
    fingerprint: string;
    iroh_endpoint_id?: string | null;
    routes?: PeerRoute[];
  };
  peers: PeerDevice[];
  paired_peer_endpoints: Record<string, string>;
  discovery_error?: string | null;
}

export interface PairingPeerStatus {
  hostname: string;
  address: string;
  fingerprint: string;
  verification_code: string;
  local_confirmed: boolean;
  remote_confirmed: boolean;
}

export interface PairingStatus {
  pairing_enabled: boolean;
  phase:
    | "disabled"
    | "waiting"
    | "handshaking"
    | "verification"
    | "waiting_for_peer"
    | "paired"
    | "cancelled"
    | "timed_out"
    | "locked";
  expires_at?: number | null;
  remaining_seconds: number;
  failed_attempts: number;
  max_failures: number;
  peer?: PairingPeerStatus | null;
  error?: string | null;
}

export interface ConnectionTestResult {
  latency_ms: number;
  path?: "tcp" | "direct" | "relay";
}

export function getPeers(): Promise<PeersResponse> {
  return invoke<PeersResponse>("get_peers");
}

export function refreshPeers(): Promise<PeersResponse> {
  return invoke<PeersResponse>("refresh_peers");
}

export function togglePeer(hostname: string, enabled: boolean): Promise<void> {
  return invoke<void>("toggle_peer", { hostname, enabled });
}

export function forgetPeer(hostname: string): Promise<void> {
  return invoke<void>("forget_peer", { hostname });
}

export function testConnection(address: string): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("test_connection", { address });
}

export function enablePairing(): Promise<PairingStatus> {
  return invoke<PairingStatus>("enable_pairing");
}

export function getPairingStatus(): Promise<PairingStatus> {
  return invoke<PairingStatus>("get_pairing_status");
}

export function startPairing(address: string): Promise<PairingStatus> {
  return invoke<PairingStatus>("start_pairing", { address });
}

export function cancelPairing(): Promise<PairingStatus> {
  return invoke<PairingStatus>("cancel_pairing");
}

export function confirmPairing(): Promise<PairingStatus> {
  return invoke<PairingStatus>("confirm_pairing");
}

// ---------------------------------------------------------------------------
// Settings — write side (T246)
// ---------------------------------------------------------------------------

export function updateSettings(next: Partial<SettingsData>): Promise<void> {
  return invoke<void>("update_settings", { settingsJson: JSON.stringify(next) });
}

export function setSyncEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("set_sync_enabled", { enabled });
}

export function setSyncShortcut(shortcut: string): Promise<void> {
  return invoke<void>("set_sync_shortcut", { shortcut });
}

export function setHistoryShortcut(shortcut: string): Promise<void> {
  return invoke<void>("set_history_shortcut", { shortcut });
}

export function suspendSyncShortcut(): Promise<void> {
  return invoke<void>("suspend_sync_shortcut");
}

export function resumeSyncShortcut(): Promise<void> {
  return invoke<void>("resume_sync_shortcut");
}

// ---------------------------------------------------------------------------
// Custom themes (T005)
// ---------------------------------------------------------------------------

/** One palette colour: a `#rrggbb` hex plus an optional opacity. */
export interface ThemeColorSpec {
  hex: string;
  opacity?: number | null;
}

/** The 24 colour tokens of THEMING.md §2.2 (camelCase field names). */
export interface ThemePalette {
  brand: ThemeColorSpec;
  brandHover: ThemeColorSpec;
  brandSoft: ThemeColorSpec;
  brandText: ThemeColorSpec;
  bgWindow: ThemeColorSpec;
  bgCard: ThemeColorSpec;
  bgInput: ThemeColorSpec;
  bgHover: ThemeColorSpec;
  bgActive: ThemeColorSpec;
  bgRaised: ThemeColorSpec;
  bgToast: ThemeColorSpec;
  textPrimary: ThemeColorSpec;
  textSecondary: ThemeColorSpec;
  textTertiary: ThemeColorSpec;
  textToast: ThemeColorSpec;
  border: ThemeColorSpec;
  borderStrong: ThemeColorSpec;
  divider: ThemeColorSpec;
  green: ThemeColorSpec;
  greenSoft: ThemeColorSpec;
  orange: ThemeColorSpec;
  orangeSoft: ThemeColorSpec;
  purple: ThemeColorSpec;
  purpleSoft: ThemeColorSpec;
}

export interface ThemeMetrics {
  cardRadius: number;
  controlRadius: number;
  rowPadding: number;
  shadowRadius: number;
}

export interface ThemeTypography {
  sectionTitleSize: number;
  uppercasesSectionTitles: boolean;
  searchSize: number;
  searchUsesDisplayFont: boolean;
  historyContentSize: number;
}

export interface ThemeFonts {
  display?: string | null;
  reading?: string | null;
}

export interface ThemeStructural {
  borderRadius?: number | null;
  shadow?: boolean | null;
  [key: string]: unknown;
}

/** One validated custom theme as returned by the daemon. */
export interface ThemeEntry {
  id: string;
  name: Record<string, string>;
  file: string;
  palette: { light: ThemePalette; dark: ThemePalette };
  metrics: ThemeMetrics;
  typography: ThemeTypography;
  fonts: ThemeFonts;
  structural?: ThemeStructural | null;
  background?: ThemeEntryBackground | null;
}

/** Background metadata per theme entry: presence/scrim/MIME only — never
 * image bytes (the payload is fetched on demand via getThemeBackground). */
export interface ThemeEntryBackground {
  light?: ThemeBackgroundMeta | null;
  dark?: ThemeBackgroundMeta | null;
}

export interface ThemeBackgroundMeta {
  hasImage: boolean;
  scrim?: ThemeColorSpec | null;
  mimeType?: string | null;
}

/** Decoded background image payload as returned by the daemon (validated
 * bytes + validated MIME type). */
export interface ThemeBackgroundPayload {
  mimeType: string;
  dataB64: string;
}

/** Error marker for a theme file that was skipped by the daemon. */
export interface ThemeErrorItem {
  file: string;
  reason: string;
}

export interface ThemesListing {
  builtin: { id: string }[];
  custom: ThemeEntry[];
  errors: ThemeErrorItem[];
}

export function listThemes(): Promise<ThemesListing> {
  return invoke<ThemesListing>("list_themes");
}

export function getThemeBackground(
  themeId: string,
  mode: "light" | "dark",
): Promise<ThemeBackgroundPayload | null> {
  return invoke<ThemeBackgroundPayload | null>("get_theme_background", { themeId, mode });
}

export function importTheme(path: string): Promise<ThemeEntry> {
  return invoke<ThemeEntry>("import_theme", { path });
}

export function deleteTheme(themeId: string): Promise<void> {
  return invoke<void>("delete_theme", { themeId });
}

export function revealThemesDir(): Promise<void> {
  return invoke<void>("reveal_themes_dir");
}
