import { describe, expect, it } from "vitest";
import type { HistoryCategory, HistoryEntry } from "../../tailsyncClient";
import { CATEGORY_FILTERS } from "./HistoryConstants";
import { resolvedCategories, resolvedCategory } from "./HistoryEntryHelpers";

describe("unified code history category", () => {
  it.each<HistoryCategory>(["command", "structured_data", "code"])("maps saved %s labels to one code label", (category) => {
    const entry: HistoryEntry = {
      id: 7, timestamp: "2026-10-10T00:00:00Z", type: "text",
      description: "git status --short", data_hash: "fixture", size_bytes: 18, source_peer: "self",
      category, categories: ["command", "structured_data", "code"],
    };
    expect(resolvedCategory(entry)).toBe("code");
    expect(resolvedCategories(entry)).toEqual(["code"]);
  });

  it("offers a single code filter", () => {
    expect(CATEGORY_FILTERS).toEqual(["all", "text", "website", "code", "path", "image", "file"]);
  });
});
