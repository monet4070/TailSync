// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.
const isRecord = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);

export type ActiveRoute = {
  "address": string;
  "interface": ConnectionInterface;
  "latency": number;
};
function validActiveRoute(value: unknown): value is ActiveRoute { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (Object.hasOwn(value, "latency") && (typeof value["latency"] === "number" && Number.isSafeInteger(value["latency"]) && value["latency"] >= 0))); }
export function decodeActiveRoute(value: unknown): ActiveRoute { if (!validActiveRoute(value)) throw new Error("Invalid ActiveRoute response"); return value; }

export type ConnectionInterface = "lan" | "iroh" | "tailscale";
function validConnectionInterface(value: unknown): value is ConnectionInterface { return (typeof value === "string" && (value === "lan" || value === "iroh" || value === "tailscale")); }
export function decodeConnectionInterface(value: unknown): ConnectionInterface { if (!validConnectionInterface(value)) throw new Error("Invalid ConnectionInterface response"); return value; }

export type DaemonStatus = {
  "active_routes": Record<string, ActiveRoute>;
  "clipboard_monitor_failures": number;
  "clipboard_monitor_healthy": boolean;
  "tcp_server_healthy": boolean;
};
function validDaemonStatus(value: unknown): value is DaemonStatus { return (isRecord(value) && (Object.hasOwn(value, "active_routes") && (isRecord(value["active_routes"]) && Object.values(value["active_routes"]).every(item => validActiveRoute(item)))) && (Object.hasOwn(value, "clipboard_monitor_failures") && (typeof value["clipboard_monitor_failures"] === "number" && Number.isSafeInteger(value["clipboard_monitor_failures"]) && value["clipboard_monitor_failures"] >= 0)) && (Object.hasOwn(value, "clipboard_monitor_healthy") && (typeof value["clipboard_monitor_healthy"] === "boolean")) && (Object.hasOwn(value, "tcp_server_healthy") && (typeof value["tcp_server_healthy"] === "boolean"))); }
export function decodeDaemonStatus(value: unknown): DaemonStatus { if (!validDaemonStatus(value)) throw new Error("Invalid DaemonStatus response"); return value; }

export type FavoriteMutation = {
  "affected_ids": Array<number>;
  "favorite": boolean;
};
function validFavoriteMutation(value: unknown): value is FavoriteMutation { return (isRecord(value) && (Object.hasOwn(value, "affected_ids") && (Array.isArray(value["affected_ids"]) && value["affected_ids"].every(item => (typeof item === "number" && Number.isSafeInteger(item))))) && (Object.hasOwn(value, "favorite") && (typeof value["favorite"] === "boolean"))); }
export function decodeFavoriteMutation(value: unknown): FavoriteMutation { if (!validFavoriteMutation(value)) throw new Error("Invalid FavoriteMutation response"); return value; }

export type FileProgress = {
  "active": boolean;
  "batch_id": string;
  "can_stop": boolean;
  "completed_files": number;
  "device": string;
  "direction": "sending" | "receiving";
  "name": string;
  "sent": number;
  "speed_bytes_per_second": number;
  "status": string;
  "total": number;
  "total_files": number;
};
function validFileProgress(value: unknown): value is FileProgress { return (isRecord(value) && (Object.hasOwn(value, "active") && (typeof value["active"] === "boolean")) && (Object.hasOwn(value, "batch_id") && (typeof value["batch_id"] === "string")) && (Object.hasOwn(value, "can_stop") && (typeof value["can_stop"] === "boolean")) && (Object.hasOwn(value, "completed_files") && (typeof value["completed_files"] === "number" && Number.isSafeInteger(value["completed_files"]) && value["completed_files"] >= 0)) && (Object.hasOwn(value, "device") && (typeof value["device"] === "string")) && (Object.hasOwn(value, "direction") && (typeof value["direction"] === "string" && (value["direction"] === "sending" || value["direction"] === "receiving"))) && (Object.hasOwn(value, "name") && (typeof value["name"] === "string")) && (Object.hasOwn(value, "sent") && (typeof value["sent"] === "number" && Number.isSafeInteger(value["sent"]) && value["sent"] >= 0)) && (Object.hasOwn(value, "speed_bytes_per_second") && (typeof value["speed_bytes_per_second"] === "number" && Number.isSafeInteger(value["speed_bytes_per_second"]) && value["speed_bytes_per_second"] >= 0)) && (Object.hasOwn(value, "status") && (typeof value["status"] === "string")) && (Object.hasOwn(value, "total") && (typeof value["total"] === "number" && Number.isSafeInteger(value["total"]) && value["total"] >= 0)) && (Object.hasOwn(value, "total_files") && (typeof value["total_files"] === "number" && Number.isSafeInteger(value["total_files"]) && value["total_files"] >= 0))); }
export function decodeFileProgress(value: unknown): FileProgress { if (!validFileProgress(value)) throw new Error("Invalid FileProgress response"); return value; }

export type HistoryEntry = {
  "batch_count": number | null;
  "batch_id": string | null;
  "batch_index": number | null;
  "batch_status": "complete" | "incomplete";
  "batch_total": number | null;
  "categories": Array<string>;
  "category": "text" | "website" | "code" | "command" | "structured_data" | "path" | "image" | "file";
  "category_confidence": number;
  "classifier_version": number;
  "data_hash": string;
  "description": string;
  "id": number;
  "pinned": boolean;
  "size_bytes": number;
  "source_peer": string;
  "timestamp": string;
  "type": PreviewKind;
};
function validHistoryEntry(value: unknown): value is HistoryEntry { return (isRecord(value) && (Object.hasOwn(value, "batch_count") && ((typeof value["batch_count"] === "number" && Number.isSafeInteger(value["batch_count"])) || (value["batch_count"] === null))) && (Object.hasOwn(value, "batch_id") && ((typeof value["batch_id"] === "string") || (value["batch_id"] === null))) && (Object.hasOwn(value, "batch_index") && ((typeof value["batch_index"] === "number" && Number.isSafeInteger(value["batch_index"])) || (value["batch_index"] === null))) && (Object.hasOwn(value, "batch_status") && (typeof value["batch_status"] === "string" && (value["batch_status"] === "complete" || value["batch_status"] === "incomplete"))) && (Object.hasOwn(value, "batch_total") && ((typeof value["batch_total"] === "number" && Number.isSafeInteger(value["batch_total"])) || (value["batch_total"] === null))) && (Object.hasOwn(value, "categories") && (Array.isArray(value["categories"]) && value["categories"].every(item => (typeof item === "string")))) && (Object.hasOwn(value, "category") && (typeof value["category"] === "string" && (value["category"] === "text" || value["category"] === "website" || value["category"] === "code" || value["category"] === "command" || value["category"] === "structured_data" || value["category"] === "path" || value["category"] === "image" || value["category"] === "file"))) && (Object.hasOwn(value, "category_confidence") && (typeof value["category_confidence"] === "number" && Number.isSafeInteger(value["category_confidence"]))) && (Object.hasOwn(value, "classifier_version") && (typeof value["classifier_version"] === "number" && Number.isSafeInteger(value["classifier_version"]))) && (Object.hasOwn(value, "data_hash") && (typeof value["data_hash"] === "string")) && (Object.hasOwn(value, "description") && (typeof value["description"] === "string")) && (Object.hasOwn(value, "id") && (typeof value["id"] === "number" && Number.isSafeInteger(value["id"]))) && (Object.hasOwn(value, "pinned") && (typeof value["pinned"] === "boolean")) && (Object.hasOwn(value, "size_bytes") && (typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]))) && (Object.hasOwn(value, "source_peer") && (typeof value["source_peer"] === "string")) && (Object.hasOwn(value, "timestamp") && (typeof value["timestamp"] === "string")) && (Object.hasOwn(value, "type") && validPreviewKind(value["type"]))); }
export function decodeHistoryEntry(value: unknown): HistoryEntry { if (!validHistoryEntry(value)) throw new Error("Invalid HistoryEntry response"); return value; }

export type HistoryQueryPage = {
  "entries": Array<HistoryEntry>;
  "has_more": boolean;
  "total": number | null;
};
function validHistoryQueryPage(value: unknown): value is HistoryQueryPage { return (isRecord(value) && (Object.hasOwn(value, "entries") && (Array.isArray(value["entries"]) && value["entries"].every(item => validHistoryEntry(item)))) && (Object.hasOwn(value, "has_more") && (typeof value["has_more"] === "boolean")) && (Object.hasOwn(value, "total") && ((typeof value["total"] === "number" && Number.isSafeInteger(value["total"]) && value["total"] >= 0) || (value["total"] === null)))); }
export function decodeHistoryQueryPage(value: unknown): HistoryQueryPage { if (!validHistoryQueryPage(value)) throw new Error("Invalid HistoryQueryPage response"); return value; }

export type LocalCapabilities = {
  "max_preview_bytes": number;
  "platform": string;
  "schema_version": number;
  "supports_binary_preview": boolean;
  "supports_runtime_snapshot": boolean;
  "supports_stable_errors": boolean;
  "wire_version": number;
};
function validLocalCapabilities(value: unknown): value is LocalCapabilities { return (isRecord(value) && (Object.hasOwn(value, "max_preview_bytes") && (typeof value["max_preview_bytes"] === "number" && Number.isSafeInteger(value["max_preview_bytes"]) && value["max_preview_bytes"] === 67108864)) && (Object.hasOwn(value, "platform") && (typeof value["platform"] === "string")) && (Object.hasOwn(value, "schema_version") && (typeof value["schema_version"] === "number" && Number.isSafeInteger(value["schema_version"]) && value["schema_version"] === 1)) && (Object.hasOwn(value, "supports_binary_preview") && (typeof value["supports_binary_preview"] === "boolean")) && (Object.hasOwn(value, "supports_runtime_snapshot") && (typeof value["supports_runtime_snapshot"] === "boolean")) && (Object.hasOwn(value, "supports_stable_errors") && (typeof value["supports_stable_errors"] === "boolean")) && (Object.hasOwn(value, "wire_version") && (typeof value["wire_version"] === "number" && Number.isSafeInteger(value["wire_version"]) && value["wire_version"] === 4))); }
export function decodeLocalCapabilities(value: unknown): LocalCapabilities { if (!validLocalCapabilities(value)) throw new Error("Invalid LocalCapabilities response"); return value; }

export type LocalDeviceSnapshot = {
  "connection_mode": string;
  "fingerprint": string;
  "hostname": string;
  "iroh_endpoint_id": string | null;
  "public_key": string;
  "routes": Array<PeerRouteSnapshot>;
  "tailscale_ip": string;
};
function validLocalDeviceSnapshot(value: unknown): value is LocalDeviceSnapshot { return (isRecord(value) && (Object.hasOwn(value, "connection_mode") && (typeof value["connection_mode"] === "string")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "iroh_endpoint_id") && ((typeof value["iroh_endpoint_id"] === "string") || (value["iroh_endpoint_id"] === null))) && (Object.hasOwn(value, "public_key") && (typeof value["public_key"] === "string")) && (Object.hasOwn(value, "routes") && (Array.isArray(value["routes"]) && value["routes"].every(item => validPeerRouteSnapshot(item)))) && (Object.hasOwn(value, "tailscale_ip") && (typeof value["tailscale_ip"] === "string"))); }
export function decodeLocalDeviceSnapshot(value: unknown): LocalDeviceSnapshot { if (!validLocalDeviceSnapshot(value)) throw new Error("Invalid LocalDeviceSnapshot response"); return value; }

export type MacRuntimeSnapshot = {
  "history_version": number;
  "notifications": Array<RuntimeNotification>;
  "progress": FileProgress | null;
  "revision": number;
  "status": DaemonStatus;
  "storage": StorageStatus;
  "sync_enabled": boolean;
};
function validMacRuntimeSnapshot(value: unknown): value is MacRuntimeSnapshot { return (isRecord(value) && (Object.hasOwn(value, "history_version") && (typeof value["history_version"] === "number" && Number.isSafeInteger(value["history_version"]) && value["history_version"] >= 0)) && (Object.hasOwn(value, "notifications") && (Array.isArray(value["notifications"]) && value["notifications"].every(item => validRuntimeNotification(item)))) && (Object.hasOwn(value, "progress") && (validFileProgress(value["progress"]) || (value["progress"] === null))) && (Object.hasOwn(value, "revision") && (typeof value["revision"] === "number" && Number.isSafeInteger(value["revision"]) && value["revision"] >= 0)) && (Object.hasOwn(value, "status") && validDaemonStatus(value["status"])) && (Object.hasOwn(value, "storage") && validStorageStatus(value["storage"])) && (Object.hasOwn(value, "sync_enabled") && (typeof value["sync_enabled"] === "boolean"))); }
export function decodeMacRuntimeSnapshot(value: unknown): MacRuntimeSnapshot { if (!validMacRuntimeSnapshot(value)) throw new Error("Invalid MacRuntimeSnapshot response"); return value; }

export type PeerCandidate = {
  "address": string;
  "interface": ConnectionInterface;
  "latency"?: number | null;
  "online": boolean;
  "priority": number;
  "rtt_capable": boolean;
  "status": PeerStatus;
};
function validPeerCandidate(value: unknown): value is PeerCandidate { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (!Object.hasOwn(value, "latency") || ((typeof value["latency"] === "number" && Number.isSafeInteger(value["latency"]) && value["latency"] >= 0) || (value["latency"] === null))) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "priority") && (typeof value["priority"] === "number" && Number.isSafeInteger(value["priority"]) && value["priority"] >= 0 && value["priority"] <= 255)) && (Object.hasOwn(value, "rtt_capable") && (typeof value["rtt_capable"] === "boolean")) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"]))); }
export function decodePeerCandidate(value: unknown): PeerCandidate { if (!validPeerCandidate(value)) throw new Error("Invalid PeerCandidate response"); return value; }

export type PeerRouteSnapshot = {
  "address": string;
  "connected": boolean;
  "interface": ConnectionInterface;
  "latency_ms": number | null;
  "online": boolean;
  "pairing_endpoint": boolean;
  "rtt_capable": boolean;
  "status": PeerStatus;
};
function validPeerRouteSnapshot(value: unknown): value is PeerRouteSnapshot { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "connected") && (typeof value["connected"] === "boolean")) && (Object.hasOwn(value, "interface") && validConnectionInterface(value["interface"])) && (Object.hasOwn(value, "latency_ms") && ((typeof value["latency_ms"] === "number" && Number.isSafeInteger(value["latency_ms"]) && value["latency_ms"] >= 0) || (value["latency_ms"] === null))) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "pairing_endpoint") && (typeof value["pairing_endpoint"] === "boolean")) && (Object.hasOwn(value, "rtt_capable") && (typeof value["rtt_capable"] === "boolean")) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"]))); }
export function decodePeerRouteSnapshot(value: unknown): PeerRouteSnapshot { if (!validPeerRouteSnapshot(value)) throw new Error("Invalid PeerRouteSnapshot response"); return value; }

export type PeerSnapshot = {
  "address": string;
  "candidates": Array<PeerCandidate>;
  "connection_mode": string;
  "current_address"?: string | null;
  "current_interface"?: ConnectionInterface | null;
  "enabled": boolean;
  "fingerprint": string;
  "hostname": string;
  "online": boolean;
  "protocol_error": string | null;
  "required_protocol_version": number | null;
  "routes": Array<PeerRouteSnapshot>;
  "status": PeerStatus;
  "tailscale_ip": string;
  "trusted": boolean;
};
function validPeerSnapshot(value: unknown): value is PeerSnapshot { return (isRecord(value) && (Object.hasOwn(value, "address") && (typeof value["address"] === "string")) && (Object.hasOwn(value, "candidates") && (Array.isArray(value["candidates"]) && value["candidates"].every(item => validPeerCandidate(item)))) && (Object.hasOwn(value, "connection_mode") && (typeof value["connection_mode"] === "string")) && (!Object.hasOwn(value, "current_address") || ((typeof value["current_address"] === "string") || (value["current_address"] === null))) && (!Object.hasOwn(value, "current_interface") || (validConnectionInterface(value["current_interface"]) || (value["current_interface"] === null))) && (Object.hasOwn(value, "enabled") && (typeof value["enabled"] === "boolean")) && (Object.hasOwn(value, "fingerprint") && (typeof value["fingerprint"] === "string")) && (Object.hasOwn(value, "hostname") && (typeof value["hostname"] === "string")) && (Object.hasOwn(value, "online") && (typeof value["online"] === "boolean")) && (Object.hasOwn(value, "protocol_error") && ((typeof value["protocol_error"] === "string") || (value["protocol_error"] === null))) && (Object.hasOwn(value, "required_protocol_version") && ((typeof value["required_protocol_version"] === "number" && Number.isSafeInteger(value["required_protocol_version"]) && value["required_protocol_version"] >= 0 && value["required_protocol_version"] <= 255) || (value["required_protocol_version"] === null))) && (Object.hasOwn(value, "routes") && (Array.isArray(value["routes"]) && value["routes"].every(item => validPeerRouteSnapshot(item)))) && (Object.hasOwn(value, "status") && validPeerStatus(value["status"])) && (Object.hasOwn(value, "tailscale_ip") && (typeof value["tailscale_ip"] === "string")) && (Object.hasOwn(value, "trusted") && (typeof value["trusted"] === "boolean"))); }
export function decodePeerSnapshot(value: unknown): PeerSnapshot { if (!validPeerSnapshot(value)) throw new Error("Invalid PeerSnapshot response"); return value; }

export type PeerStatus = "discovered" | "confirming" | "online" | "connected" | "offline";
function validPeerStatus(value: unknown): value is PeerStatus { return (typeof value === "string" && (value === "discovered" || value === "confirming" || value === "online" || value === "connected" || value === "offline")); }
export function decodePeerStatus(value: unknown): PeerStatus { if (!validPeerStatus(value)) throw new Error("Invalid PeerStatus response"); return value; }

export type PeersResponse = {
  "discovery_error": string | null;
  "paired_peer_endpoints": Record<string, string>;
  "peers": Array<PeerSnapshot>;
  "self": LocalDeviceSnapshot;
};
function validPeersResponse(value: unknown): value is PeersResponse { return (isRecord(value) && (Object.hasOwn(value, "discovery_error") && ((typeof value["discovery_error"] === "string") || (value["discovery_error"] === null))) && (Object.hasOwn(value, "paired_peer_endpoints") && (isRecord(value["paired_peer_endpoints"]) && Object.values(value["paired_peer_endpoints"]).every(item => (typeof item === "string")))) && (Object.hasOwn(value, "peers") && (Array.isArray(value["peers"]) && value["peers"].every(item => validPeerSnapshot(item)))) && (Object.hasOwn(value, "self") && validLocalDeviceSnapshot(value["self"]))); }
export function decodePeersResponse(value: unknown): PeersResponse { if (!validPeersResponse(value)) throw new Error("Invalid PeersResponse response"); return value; }

export type PreviewBatchNavigation = {
  "batch_id": string;
  "first_entry_id": number;
  "item_count": number;
  "item_index": number;
  "last_entry_id": number;
  "next_entry_id": number | null;
  "previous_entry_id": number | null;
};
function validPreviewBatchNavigation(value: unknown): value is PreviewBatchNavigation { return (isRecord(value) && (Object.hasOwn(value, "batch_id") && (typeof value["batch_id"] === "string")) && (Object.hasOwn(value, "first_entry_id") && (typeof value["first_entry_id"] === "number" && Number.isSafeInteger(value["first_entry_id"]))) && (Object.hasOwn(value, "item_count") && (typeof value["item_count"] === "number" && Number.isSafeInteger(value["item_count"]) && value["item_count"] >= 0)) && (Object.hasOwn(value, "item_index") && (typeof value["item_index"] === "number" && Number.isSafeInteger(value["item_index"]) && value["item_index"] >= 0)) && (Object.hasOwn(value, "last_entry_id") && (typeof value["last_entry_id"] === "number" && Number.isSafeInteger(value["last_entry_id"]))) && (Object.hasOwn(value, "next_entry_id") && ((typeof value["next_entry_id"] === "number" && Number.isSafeInteger(value["next_entry_id"])) || (value["next_entry_id"] === null))) && (Object.hasOwn(value, "previous_entry_id") && ((typeof value["previous_entry_id"] === "number" && Number.isSafeInteger(value["previous_entry_id"])) || (value["previous_entry_id"] === null)))); }
export function decodePreviewBatchNavigation(value: unknown): PreviewBatchNavigation { if (!validPreviewBatchNavigation(value)) throw new Error("Invalid PreviewBatchNavigation response"); return value; }

export type PreviewErrorCode = "entry_not_found" | "batch_not_found" | "entry_not_in_batch" | "metadata_unavailable" | "payload_unavailable" | "preview_too_large" | "unsupported_type" | "invalid_size";
function validPreviewErrorCode(value: unknown): value is PreviewErrorCode { return (typeof value === "string" && (value === "entry_not_found" || value === "batch_not_found" || value === "entry_not_in_batch" || value === "metadata_unavailable" || value === "payload_unavailable" || value === "preview_too_large" || value === "unsupported_type" || value === "invalid_size")); }
export function decodePreviewErrorCode(value: unknown): PreviewErrorCode { if (!validPreviewErrorCode(value)) throw new Error("Invalid PreviewErrorCode response"); return value; }

export type PreviewErrorInfo = {
  "code": PreviewErrorCode;
  "entry_id": number | null;
  "limit_bytes": number | null;
  "message": string;
  "retryable": boolean;
  "size_bytes": number | null;
};
function validPreviewErrorInfo(value: unknown): value is PreviewErrorInfo { return (isRecord(value) && (Object.hasOwn(value, "code") && validPreviewErrorCode(value["code"])) && (Object.hasOwn(value, "entry_id") && ((typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"])) || (value["entry_id"] === null))) && (Object.hasOwn(value, "limit_bytes") && ((typeof value["limit_bytes"] === "number" && Number.isSafeInteger(value["limit_bytes"]) && value["limit_bytes"] >= 0) || (value["limit_bytes"] === null))) && (Object.hasOwn(value, "message") && (typeof value["message"] === "string")) && (Object.hasOwn(value, "retryable") && (typeof value["retryable"] === "boolean")) && (Object.hasOwn(value, "size_bytes") && ((typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]) && value["size_bytes"] >= 0) || (value["size_bytes"] === null)))); }
export function decodePreviewErrorInfo(value: unknown): PreviewErrorInfo { if (!validPreviewErrorInfo(value)) throw new Error("Invalid PreviewErrorInfo response"); return value; }

export type PreviewFrameMetadata = {
  "batch": PreviewBatchNavigation | null;
  "entry_id": number;
  "height": number | null;
  "kind": "text" | "image" | "file";
  "name": string;
  "request_id"?: string | null;
  "size_bytes": number;
  "width": number | null;
};
function validPreviewFrameMetadata(value: unknown): value is PreviewFrameMetadata { return (isRecord(value) && (Object.hasOwn(value, "batch") && (validPreviewBatchNavigation(value["batch"]) || (value["batch"] === null))) && (Object.hasOwn(value, "entry_id") && (typeof value["entry_id"] === "number" && Number.isSafeInteger(value["entry_id"]) && value["entry_id"] >= 1)) && (Object.hasOwn(value, "height") && ((typeof value["height"] === "number" && Number.isSafeInteger(value["height"]) && value["height"] >= 0) || (value["height"] === null))) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string" && (value["kind"] === "text" || value["kind"] === "image" || value["kind"] === "file"))) && (Object.hasOwn(value, "name") && (typeof value["name"] === "string" && value["name"].length >= 1)) && (!Object.hasOwn(value, "request_id") || ((typeof value["request_id"] === "string") || (value["request_id"] === null))) && (Object.hasOwn(value, "size_bytes") && (typeof value["size_bytes"] === "number" && Number.isSafeInteger(value["size_bytes"]) && value["size_bytes"] >= 0 && value["size_bytes"] <= 67108864)) && (Object.hasOwn(value, "width") && ((typeof value["width"] === "number" && Number.isSafeInteger(value["width"]) && value["width"] >= 0) || (value["width"] === null)))); }
export function decodePreviewFrameMetadata(value: unknown): PreviewFrameMetadata { if (!validPreviewFrameMetadata(value)) throw new Error("Invalid PreviewFrameMetadata response"); return value; }

export type PreviewKind = "text" | "image" | "file";
function validPreviewKind(value: unknown): value is PreviewKind { return (typeof value === "string" && (value === "text" || value === "image" || value === "file")); }
export function decodePreviewKind(value: unknown): PreviewKind { if (!validPreviewKind(value)) throw new Error("Invalid PreviewKind response"); return value; }

export type RuntimeNotification = {
  "id": number;
  "level": string;
  "message": string;
};
function validRuntimeNotification(value: unknown): value is RuntimeNotification { return (isRecord(value) && (Object.hasOwn(value, "id") && (typeof value["id"] === "number" && Number.isSafeInteger(value["id"]) && value["id"] >= 0)) && (Object.hasOwn(value, "level") && (typeof value["level"] === "string")) && (Object.hasOwn(value, "message") && (typeof value["message"] === "string"))); }
export function decodeRuntimeNotification(value: unknown): RuntimeNotification { if (!validRuntimeNotification(value)) throw new Error("Invalid RuntimeNotification response"); return value; }

export type StableErrorCode = "invalid_argument" | "not_found" | "temporarily_busy" | "storage_unavailable" | "unauthorized" | "protocol_incompatible" | "internal_error";
function validStableErrorCode(value: unknown): value is StableErrorCode { return typeof value === "string"; }
export function decodeStableErrorCode(value: unknown): StableErrorCode { if (!validStableErrorCode(value)) throw new Error("Invalid StableErrorCode response"); return ["invalid_argument","not_found","temporarily_busy","storage_unavailable","unauthorized","protocol_incompatible","internal_error"].includes(value as StableErrorCode) ? value as StableErrorCode : "internal_error"; }

export type StableErrorDetailClass = "request" | "resource" | "contention" | "storage" | "authorization" | "protocol" | "internal";
function validStableErrorDetailClass(value: unknown): value is StableErrorDetailClass { return (typeof value === "string" && (value === "request" || value === "resource" || value === "contention" || value === "storage" || value === "authorization" || value === "protocol" || value === "internal")); }
export function decodeStableErrorDetailClass(value: unknown): StableErrorDetailClass { if (!validStableErrorDetailClass(value)) throw new Error("Invalid StableErrorDetailClass response"); return value; }

export type StableErrorEnvelope = {
  "code": StableErrorCode;
  "detail_class": StableErrorDetailClass;
  "message_key": string;
  "retryable": boolean;
  "schema_version": number;
};
function validStableErrorEnvelope(value: unknown): value is StableErrorEnvelope { return (isRecord(value) && (Object.hasOwn(value, "code") && validStableErrorCode(value["code"])) && (Object.hasOwn(value, "detail_class") && validStableErrorDetailClass(value["detail_class"])) && (Object.hasOwn(value, "message_key") && (typeof value["message_key"] === "string")) && (Object.hasOwn(value, "retryable") && (typeof value["retryable"] === "boolean")) && (Object.hasOwn(value, "schema_version") && (typeof value["schema_version"] === "number" && Number.isSafeInteger(value["schema_version"]) && value["schema_version"] === 1))); }
export function decodeStableErrorEnvelope(value: unknown): StableErrorEnvelope { if (!validStableErrorEnvelope(value)) throw new Error("Invalid StableErrorEnvelope response"); const code = decodeStableErrorCode(value.code); return code === "internal_error" && value.code !== "internal_error" ? { ...value, code, retryable: false, message_key: "error.internal", detail_class: "internal" } : value; }

export type StorageStatus = {
  "available": boolean;
  "error": string | null;
  "quota_bytes": number;
  "root": string;
  "used_bytes": number;
};
function validStorageStatus(value: unknown): value is StorageStatus { return (isRecord(value) && (Object.hasOwn(value, "available") && (typeof value["available"] === "boolean")) && (Object.hasOwn(value, "error") && ((typeof value["error"] === "string") || (value["error"] === null))) && (Object.hasOwn(value, "quota_bytes") && (typeof value["quota_bytes"] === "number" && Number.isSafeInteger(value["quota_bytes"]) && value["quota_bytes"] >= 0)) && (Object.hasOwn(value, "root") && (typeof value["root"] === "string")) && (Object.hasOwn(value, "used_bytes") && (typeof value["used_bytes"] === "number" && Number.isSafeInteger(value["used_bytes"]) && value["used_bytes"] >= 0))); }
export function decodeStorageStatus(value: unknown): StorageStatus { if (!validStorageStatus(value)) throw new Error("Invalid StorageStatus response"); return value; }

export type SyncWarning = {
  "kind": string;
  "occurred_at_ms": number;
  "peer": string;
};
function validSyncWarning(value: unknown): value is SyncWarning { return (isRecord(value) && (Object.hasOwn(value, "kind") && (typeof value["kind"] === "string")) && (Object.hasOwn(value, "occurred_at_ms") && (typeof value["occurred_at_ms"] === "number" && Number.isSafeInteger(value["occurred_at_ms"]))) && (Object.hasOwn(value, "peer") && (typeof value["peer"] === "string"))); }
export function decodeSyncWarning(value: unknown): SyncWarning { if (!validSyncWarning(value)) throw new Error("Invalid SyncWarning response"); return value; }

export type WindowsRuntimeSnapshot = {
  "history_version": number;
  "notifications": Array<RuntimeNotification>;
  "progress": FileProgress | null;
  "revision": number;
  "sync_warning": SyncWarning | null;
};
function validWindowsRuntimeSnapshot(value: unknown): value is WindowsRuntimeSnapshot { return (isRecord(value) && (Object.hasOwn(value, "history_version") && (typeof value["history_version"] === "number" && Number.isSafeInteger(value["history_version"]) && value["history_version"] >= 0)) && (Object.hasOwn(value, "notifications") && (Array.isArray(value["notifications"]) && value["notifications"].every(item => validRuntimeNotification(item)))) && (Object.hasOwn(value, "progress") && (validFileProgress(value["progress"]) || (value["progress"] === null))) && (Object.hasOwn(value, "revision") && (typeof value["revision"] === "number" && Number.isSafeInteger(value["revision"]) && value["revision"] >= 0)) && (Object.hasOwn(value, "sync_warning") && (validSyncWarning(value["sync_warning"]) || (value["sync_warning"] === null)))); }
export function decodeWindowsRuntimeSnapshot(value: unknown): WindowsRuntimeSnapshot { if (!validWindowsRuntimeSnapshot(value)) throw new Error("Invalid WindowsRuntimeSnapshot response"); return value; }
