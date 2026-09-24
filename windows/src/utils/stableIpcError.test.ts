import { describe, expect, it } from "vitest";
import { normalizeIpcError, StableIpcError } from "./stableIpcError";

describe("stable Tauri command errors", () => {
  it("uses the fixed policy and drops untrusted details", () => {
    const error = normalizeIpcError({
      schema_version: 1,
      code: "unauthorized",
      retryable: true,
      message_key: "private.path",
      detail_class: "future_detail",
      path: "C:\\private\\history.db",
    });
    expect(error).toBeInstanceOf(StableIpcError);
    expect(error).toMatchObject({
      code: "unauthorized",
      retryable: false,
      messageKey: "error.unauthorized",
      detailClass: "authorization",
    });
    expect(JSON.stringify(error)).not.toContain("private");
  });

  it("maps future and malformed envelopes to a safe internal error", () => {
    for (const value of [
      { schema_version: 1, code: "future_error", retryable: true, message_key: "future", detail_class: "future" },
      { schema_version: 1, code: 123 },
    ]) {
      expect(normalizeIpcError(value)).toMatchObject({ code: "internal_error", retryable: false });
    }
  });

  it("preserves legacy strings and specialized preview errors", () => {
    const preview = { code: "preview_too_large", size_bytes: 99, retryable: false };
    expect(normalizeIpcError(preview)).toBe(preview);
    expect(normalizeIpcError("legacy error")).toBe("legacy error");
  });
});
