// F10-C3: finite pairing-failure classification consumption.
//
// The Windows pairing surfaces previously reduced every rejected Tauri promise
// to `String(error)`, which dropped the finite `StableErrorCode` the shared IPC
// boundary had already normalized, and read `PairingStatus.error` as free text.
// This module keeps that finite classification (a normalized code, or the
// status's `failure_kind`) and resolves it to a fixed, localized policy key.
// Legacy source text is only used when no finite classification exists.

import {
  decodePairingFailureKind,
  decodeStableErrorEnvelope,
  type PairingFailureKind,
  type StableErrorCode,
} from "../types/localContracts.generated";
import { StableIpcError } from "./stableIpcError";

/** A pairing failure ready for rendering: a fixed policy key, or legacy text. */
export type PairingIssue =
  | { kind: "typed"; code: StableErrorCode; messageKey: string }
  | { kind: "legacy"; text: string };

export type Translate = (key: string) => string;

/**
 * Finite `PairingFailureKind` -> finite `StableErrorCode`, identical to the
 * runtime classification. Transport and invalid advertisement keep the
 * conservative internal policy: the legacy transport error is also used for
 * persisted failures, so it must not be presented as a network failure.
 */
export function stableCodeForPairingKind(kind: PairingFailureKind): StableErrorCode {
  switch (kind) {
    case "protocol_incompatible": return "protocol_incompatible";
    case "invalid_argument": return "invalid_argument";
    case "identity_mismatch": return "pairing_identity_mismatch";
    case "window_closed": return "pairing_window_closed";
    case "timed_out": return "pairing_timed_out";
    case "connection_closed": return "pairing_connection_closed";
    case "busy": return "pairing_busy";
    case "cancelled": return "pairing_cancelled";
    case "transport":
    case "invalid_advertisement":
    case "unknown":
      return "internal_error";
  }
}

/** Fixed policy key for a finite code, reusing the generated policy table. */
export function messageKeyForStableCode(code: StableErrorCode): string {
  return decodeStableErrorEnvelope({
    schema_version: 1,
    code,
    retryable: false,
    message_key: "",
    detail_class: "",
  }).message_key;
}

/**
 * Typed issue for a known finite kind. Unknown, missing, null and future values
 * return null so the caller falls back to the legacy status text.
 */
export function pairingIssueForKind(kind: unknown): PairingIssue | null {
  if (typeof kind !== "string") return null;
  const finiteKind = decodePairingFailureKind(kind);
  if (finiteKind === "unknown") return null;
  const code = stableCodeForPairingKind(finiteKind);
  return { kind: "typed", code, messageKey: messageKeyForStableCode(code) };
}

/** Typed issue for a normalized IPC error, otherwise the legacy source text. */
export function pairingIssueForError(error: unknown): PairingIssue {
  if (error instanceof StableIpcError) {
    return { kind: "typed", code: error.code, messageKey: error.messageKey };
  }
  return { kind: "legacy", text: String(error) };
}

/** Status-derived issue: a known finite kind wins over the legacy status text. */
export function pairingIssueForStatus(
  status: { failure_kind?: PairingFailureKind | null; error?: string | null } | null | undefined,
): PairingIssue | null {
  if (!status) return null;
  const typed = pairingIssueForKind(status.failure_kind);
  if (typed) return typed;
  return typeof status.error === "string" && status.error
    ? { kind: "legacy", text: status.error }
    : null;
}

/** Display text for an issue, keeping the legacy identity-mismatch fallback. */
export function pairingIssueText(issue: PairingIssue | null, t: Translate): string {
  if (!issue) return "";
  if (issue.kind === "typed") return t(issue.messageKey);
  return /identity\s+mismatch/i.test(issue.text) ? t("settings.identityMismatch") : issue.text;
}
