import en from "../i18n/en.json";
import zhCN from "../i18n/zh-CN.json";
import { decodeStableErrorEnvelope, type StableErrorEnvelope } from "../types/localContracts.generated";

const messages: Record<string, Record<string, string>> = { en, "zh-CN": zhCN };

function localMessage(key: string): string {
  let locale: string | null = null;
  try { locale = localStorage.getItem("tailsync-lang"); } catch { /* unavailable storage */ }
  locale ??= typeof navigator !== "undefined" && navigator.language.startsWith("zh") ? "zh-CN" : "en";
  return messages[locale]?.[key] ?? messages.en[key] ?? messages.en["error.internal"];
}

export class StableIpcError extends Error {
  readonly code: StableErrorEnvelope["code"];
  readonly retryable: boolean;
  readonly messageKey: string;
  readonly detailClass: StableErrorEnvelope["detail_class"];

  constructor(envelope: StableErrorEnvelope) {
    super(localMessage(envelope.message_key));
    this.name = "StableIpcError";
    this.code = envelope.code;
    this.retryable = envelope.retryable;
    this.messageKey = envelope.message_key;
    this.detailClass = envelope.detail_class;
  }
}

/** Keep specialized preview errors intact; normalize only versioned envelopes. */
export function normalizeIpcError(error: unknown): unknown {
  if (typeof error !== "object" || error === null || !("schema_version" in error)) return error;
  try {
    return new StableIpcError(decodeStableErrorEnvelope(error));
  } catch {
    return new StableIpcError(decodeStableErrorEnvelope({
      schema_version: 1,
      code: "internal_error",
      retryable: false,
      message_key: "error.internal",
      detail_class: "internal",
    }));
  }
}
