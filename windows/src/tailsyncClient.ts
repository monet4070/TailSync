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

export type HistoryCollection = "all" | "favorites";
export type PreviewWindowOwner = "history" | "favorites";

export interface FavoriteMutation {
  affected_ids: number[];
  favorite: boolean;
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
  kind: "expired_event" | "delivery_stalled" | "delivery_shutdown" | "delivery_expired";
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

export interface RuntimeSnapshot {
  revision: number;
  history_version: number;
  progress: FileProgress | null;
  sync_warning: SyncWarning | null;
}

export type HistoryPageQuery = {
  keyword: string | null;
  category: HistoryCategory | null;
  startTime: string | null;
  endTime: string | null;
  limit: number;
  offset: number;
  collection?: HistoryCollection;
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
  owner: PreviewWindowOwner;
}

export function openPreviewWindow(
  entryId: number,
  batchId?: string | null,
  owner: HistoryCollection = "all",
): Promise<PreviewWindowSnapshot> {
  return invoke<PreviewWindowSnapshot>("open_preview_window", {
    request: {
      entryId,
      batchId: batchId ?? null,
      owner: owner === "favorites" ? "favorites" : "history",
    },
  });
}

export function getPreviewWindowRequest(): Promise<PreviewWindowSnapshot | null> {
  return invoke<PreviewWindowSnapshot | null>("get_preview_window_request");
}

export function closePreviewWindow(owner?: HistoryCollection): Promise<void> {
  return owner
    ? invoke<void>("close_preview_window", {
        owner: owner === "favorites" ? "favorites" : "history",
      })
    : invoke<void>("close_preview_window");
}

export function closeHistoryWindow(): Promise<void> {
  return invoke<void>("close_history_window");
}

export function openFavoritesWindow(): Promise<void> {
  return invoke<void>("open_favorites_window");
}

export function closeFavoritesWindow(): Promise<void> {
  return invoke<void>("close_favorites_window");
}

export function closeSettingsWindow(): Promise<void> {
  return invoke<void>("close_settings_window");
}

export function syncPreviewWindowMinimized(
  minimized: boolean,
  owner: HistoryCollection = "all",
): Promise<void> {
  return invoke<void>("sync_preview_window_minimized", {
    minimized,
    owner: owner === "favorites" ? "favorites" : "history",
  });
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

export function waitRuntimeSnapshot(
  sinceRevision: number,
  waitMs = 2_500,
): Promise<RuntimeSnapshot> {
  return invoke<RuntimeSnapshot>("wait_runtime_snapshot", { sinceRevision, waitMs });
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

export function setHistoryFavorite(id: number, favorite: boolean): Promise<FavoriteMutation> {
  return invoke<FavoriteMutation>("set_history_favorite", { id, favorite });
}

export function deleteFavoriteEntry(id: number): Promise<FavoriteMutation> {
  return invoke<FavoriteMutation>("delete_favorite_entry", { id });
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
    | "finalizing"
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

// V2 resolved model. Tokens stay structured until this boundary; renderers
// receive no theme-supplied CSS or executable text.
export interface ResolvedThemeV2 {
  id: string;
  digest: string;
  mode: "light" | "dark";
  highContrast: boolean;
  tokens: Record<string, unknown>;
  provenance: Record<string, string>;
  assetSlots: Record<string, { slot: string; key: string; digest: string; mimeType: string; bytes: number; width: number; height: number }>;
}

export function resolveThemeV2(
  themeId: string,
  mode: "light" | "dark",
  highContrast = false,
): Promise<ResolvedThemeV2> {
  return invoke<ResolvedThemeV2>("resolve_theme", { themeId, mode, platform: "windows", highContrast });
}

export interface ThemeDiagnosticV2 { code: string; message: string; jsonPointer: string; severity: "error" | "warning"; platforms: string[]; recoverable: boolean; fallbackApplied: boolean; }

function themeErrorObject(value: unknown): Record<string, unknown> | undefined {
  if (typeof value === "string") {
    try { return themeErrorObject(JSON.parse(value)); } catch { return undefined; }
  }
  if (!value || typeof value !== "object") return undefined;
  return value as Record<string, unknown>;
}

/** Retains the complete Core diagnostic when a Tauri command rejects. */
export function decodeThemeDiagnostic(error: unknown): ThemeDiagnosticV2 | undefined {
  const queue: unknown[] = [error];
  const seen = new Set<object>();
  while (queue.length > 0) {
    const value = queue.shift();
    const object = themeErrorObject(value);
    if (object) {
      if (seen.has(object)) continue;
      seen.add(object);
      if (typeof object.code === "string" && typeof object.message === "string") {
        return {
          code: object.code,
          message: object.message,
          jsonPointer: typeof object.jsonPointer === "string" ? object.jsonPointer : "",
          severity: object.severity === "warning" ? "warning" : "error",
          platforms: Array.isArray(object.platforms)
            ? object.platforms.filter((platform): platform is string => typeof platform === "string")
            : [],
          recoverable: object.recoverable === true,
          fallbackApplied: object.fallbackApplied === true,
        };
      }
      queue.push(object.data, object.error, object.cause, object.message);
    }
  }
  return undefined;
}

export function formatThemeError(error: unknown): string {
  const diagnostic = decodeThemeDiagnostic(error);
  if (diagnostic) {
    const context = [
      diagnostic.jsonPointer,
      diagnostic.platforms.join(", "),
      diagnostic.recoverable ? "recoverable" : "not recoverable",
      diagnostic.fallbackApplied ? "fallback applied" : "fallback not applied",
    ].filter(Boolean).join("; ");
    return `[${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}${context ? ` (${context})` : ""}`;
  }
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  try { return JSON.stringify(error) || "Unknown theme error"; } catch { return "Unknown theme error"; }
}

export interface ThemeV2Descriptor { id: string; storageHandle: string; source: "builtin" | "custom"; version: string; digest: string; name: Record<string, string>; status: "valid" | "invalid"; resolvedLight?: ResolvedThemeV2; resolvedDark?: ResolvedThemeV2; diagnostics: ThemeDiagnosticV2[]; }
export interface LocalThemeSettingsV2 { activeThemeId: string; appearance: "light" | "dark" | "system"; highContrast: boolean; }
export interface UpdateThemeOptions { allowSameVersion?: boolean; allowDowngrade?: boolean; }
export function listThemesV2(): Promise<ThemeV2Descriptor[]> { return invoke<ThemeV2Descriptor[]>("list_themes_v2"); }
export interface ThemeValidationV2 { valid: boolean; digest?: string; candidateVersion?: string; preview?: ResolvedThemeV2; diagnostics: ThemeDiagnosticV2[]; }
export function validateThemeV2(path: string, mode: "light" | "dark" = "light", highContrast = false): Promise<ThemeValidationV2> { return invoke("validate_theme", { path, mode, highContrast }); }
export function installThemeV2(path: string, expectedDigest: string): Promise<ThemeV2Descriptor> { return invoke("install_theme", { path, expectedDigest }); }
export function updateThemeV2(path: string, expectedDigest: string, options: UpdateThemeOptions = {}): Promise<ThemeV2Descriptor> { return invoke("update_theme", { path, expectedDigest, options }); }
export function rollbackThemeV2(themeId: string): Promise<ThemeV2Descriptor> { return invoke("rollback_theme", { themeId }); }
export function deleteThemeV2(themeId: string, storageHandle?: string): Promise<void> { return invoke("delete_theme_v2", { themeId, storageHandle }); }
export function getLocalThemeSettingsV2(): Promise<LocalThemeSettingsV2> { return invoke("get_local_theme_settings"); }
export function setLocalThemeSettingsV2(settings: LocalThemeSettingsV2): Promise<void> { return invoke("set_local_theme_settings", { settings }); }
export function getThemeAssetSlot(themeId: string, digest: string, slot: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_theme_asset_slot", { themeId, digest, slot });
}
export function previewThemeAssetSlot(path: string, digest: string, slot: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("preview_theme_asset_slot", { path, digest, slot });
}
