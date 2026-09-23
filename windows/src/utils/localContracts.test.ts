import { describe, expect, it } from "vitest";
import { decodeLocalCapabilities } from "../tailsyncClient";
import * as contracts from "../types/localContracts.generated";
import fixtures from "../../../shared/schema/fixtures/local-contracts.json";

const valid = {
  schema_version: 1,
  wire_version: 4,
  platform: "windows",
  max_preview_bytes: 64 * 1024 * 1024,
  supports_binary_preview: true,
  supports_runtime_snapshot: true,
  supports_stable_errors: true,
};

describe("local IPC capabilities", () => {
  it("decodes every production Rust fixture through the TypeScript runtime decoder", () => {
    for (const fixture of fixtures) {
      const decode = Reflect.get(contracts, `decode${fixture.contract}`) as (value: unknown) => unknown;
      expect(typeof decode, fixture.name).toBe("function");
      const expected = "typescriptValid" in fixture ? fixture.typescriptValid : fixture.valid;
      if (expected) expect(() => decode(fixture.value), fixture.name).not.toThrow();
      else expect(() => decode(fixture.value), fixture.name).toThrow();
    }
  });
  it("accepts the versioned capability contract", () => {
    expect(decodeLocalCapabilities(valid)).toEqual(valid);
  });

  it("rejects a wire version or payload limit drift", () => {
    expect(() => decodeLocalCapabilities({ ...valid, wire_version: 3 })).toThrow();
    expect(() => decodeLocalCapabilities({ ...valid, max_preview_bytes: 1 })).toThrow();
  });

  it("rejects missing required flags", () => {
    const { supports_stable_errors: _ignored, ...missing } = valid;
    expect(() => decodeLocalCapabilities(missing)).toThrow();
  });

  it("maps an unknown stable error code to internal_error", () => {
    const decoded = contracts.decodeStableErrorEnvelope({
      schema_version: 1,
      code: "future_error",
      retryable: true,
      message_key: "future.error",
      detail_class: "internal",
    });
    expect(decoded.code).toBe("internal_error");
    expect(decoded.retryable).toBe(false);
    expect(decoded.message_key).toBe("error.internal");
    expect(decoded.detail_class).toBe("internal");
  });
});
