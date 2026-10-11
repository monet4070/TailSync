
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import en from "../../i18n/en.json";
import zhCN from "../../i18n/zh-CN.json";
import type { CompositeInfo } from "../../types/localContracts.generated";
import {
  explicitRestoreFormats,
  rowDiagnostics,
  rowStatusLabel,
  showsAdvancedActions,
  showsMaintenanceActions,
} from "./historyRowActions";

const translate = (key: string): string => key;

function composite(overrides: Partial<CompositeInfo> = {}): CompositeInfo {
  return {
    completeness: "complete",
    formats: ["text"],
    image_count: 0,
    native_item_count: 1,
    native_only: false,
    representation_count: 1,
    restorable_formats: ["text"],
    withheld_formats: [],
    ...overrides,
  };
}

describe("historyRowActions", () => {
  it("opens the text backup entry for a rich item and keeps it out of a plain-text row", () => {
    expect(
      explicitRestoreFormats(
        composite({ formats: ["text", "html"], restorable_formats: ["text", "html"] }),
      ),
    ).toEqual(["text"]);
    expect(
      explicitRestoreFormats(composite({ formats: ["text"], restorable_formats: ["text"] })),
    ).toEqual([]);
  });

  it("offers text and png only when the server reports them restorable", () => {
    expect(
      explicitRestoreFormats(
        composite({
          formats: ["html", "png", "text"],
          restorable_formats: ["png", "text"],
          completeness: "external_references",
        }),
      ),
    ).toEqual(["png", "text"]);
    expect(
      explicitRestoreFormats(
        composite({
          formats: ["text", "html", "png"],
          restorable_formats: ["text", "html"],
          withheld_formats: [{ format: "png", code: "no_single_image" }],
        }),
      ),
    ).toEqual(["text"]);
  });

  it("offers nothing for a native-only item", () => {
    expect(
      explicitRestoreFormats(
        composite({
          native_only: true,
          formats: ["text", "png"],
          restorable_formats: [],
          withheld_formats: [
            { format: "text", code: "native_only" },
            { format: "png", code: "native_only" },
          ],
        }),
      ),
    ).toEqual([]);
  });

  it("keeps the maintenance entries on the previous conditions", () => {
    expect(
      showsMaintenanceActions(
        composite({ formats: ["text", "html"], completeness: "external_references" }),
      ),
    ).toBe(true);
    expect(
      showsMaintenanceActions(composite({ formats: ["text", "html"], completeness: "complete" })),
    ).toBe(false);
    expect(
      showsMaintenanceActions(
        composite({ native_only: true, formats: ["html"], completeness: "missing_formats" }),
      ),
    ).toBe(false);
  });

  it("keeps the integrity wording while the counts move to details", () => {
    expect(rowStatusLabel(composite({ native_only: true }), translate)).toBe(
      "history.composite.localOnly",
    );
    expect(rowStatusLabel(composite({ completeness: "external_references" }), translate)).toBe(
      "history.composite.resourcesMissing",
    );
    expect(rowStatusLabel(composite({ completeness: "missing_formats" }), translate)).toBe(
      "history.composite.incomplete",
    );
    expect(rowStatusLabel(composite(), translate)).toBeNull();
    expect(rowStatusLabel(null, translate)).toBeNull();
    expect(rowStatusLabel(undefined, translate)).toBeNull();
  });

  it("shows the advanced submenu for an incomplete rich item or a live revoke", () => {
    expect(
      showsAdvancedActions(
        composite({ formats: ["text", "html"], restorable_formats: ["text"], completeness: "external_references" }),
        { authorize: true, importPacket: true, revoke: false },
      ),
    ).toBe(true);
    expect(
      showsAdvancedActions(
        composite({ formats: ["text", "html"], restorable_formats: ["text", "html"] }),
        { authorize: true, importPacket: true, revoke: false },
      ),
    ).toBe(false);
    expect(
      showsAdvancedActions(
        composite({ formats: ["text", "html"], restorable_formats: ["text", "html"] }),
        { authorize: false, importPacket: false, revoke: true },
      ),
    ).toBe(true);
  });

  it("exposes counts, native-only, completeness and withheld in the details rows", () => {
    const rows = rowDiagnostics(
      composite({
        formats: ["text", "html", "png"],
        restorable_formats: ["text", "html"],
        representation_count: 3,
        image_count: 2,
        completeness: "external_references",
        withheld_formats: [{ format: "png", code: "no_single_image" }],
      }),
      translate,
    );
    expect(rows).toEqual([
      { label: "history.composite.diag.formats", value: "text, html, png" },
      { label: "history.composite.diag.representations", value: "3" },
      { label: "history.composite.diag.images", value: "2" },
      { label: "history.composite.diag.nativeItems", value: "1" },
      { label: "history.composite.diag.nativeOnly", value: "history.composite.diag.no" },
      {
        label: "history.composite.diag.completeness",
        value: "history.composite.diag.unknown",
      },
      { label: "history.composite.diag.restorable", value: "text, html" },
      { label: "history.composite.diag.withheld", value: "png: history.composite.diag.unavailable" },
    ]);
  });
});

describe("history row details strings", () => {
  const keys = [
    "history.composite.moreActions",
    "history.composite.advanced",
    "history.composite.details",
    "history.composite.copy.text",
    "history.composite.copy.png",
    "history.composite.archiveImages",
    "history.composite.archiveImagesHint",
    "history.composite.resourcesMissing",
    "history.composite.diag.formats",
    "history.composite.diag.representations",
    "history.composite.diag.images",
    "history.composite.diag.nativeItems",
    "history.composite.diag.nativeOnly",
    "history.composite.diag.completeness",
    "history.composite.diag.restorable",
    "history.composite.diag.withheld",
    "history.composite.diag.yes",
    "history.composite.diag.no",
    "history.composite.diag.none",
    "history.composite.diag.unavailable",
    "history.composite.diag.unknown",
    "history.composite.completeness.complete",
    "history.composite.completeness.external_references",
    "history.composite.completeness.missing_formats",
  ];

  it("is present in both shipped languages", () => {
    const enKeys: Record<string, string> = en;
    const zhKeys: Record<string, string> = zhCN;
    for (const key of keys) {
      expect(enKeys[key], `en: ${key}`).toBeTruthy();
      expect(zhKeys[key], `zh-CN: ${key}`).toBeTruthy();
    }
  });

  it("resolves the withheld reasons the server can send", () => {
    const enKeys: Record<string, string> = en;
    const zhKeys: Record<string, string> = zhCN;
    for (const code of ["native_only", "resources_incomplete", "no_single_image"]) {
      const key = `history.restore.code.${code}`;
      expect(enKeys[key], `en: ${key}`).toBeTruthy();
      expect(zhKeys[key], `zh-CN: ${key}`).toBeTruthy();
    }
  });

  it("points resources_incomplete at the backup copy entries and ships the simplified menu labels", () => {
    const enKeys: Record<string, string> = en;
    const zhKeys: Record<string, string> = zhCN;
    expect(enKeys["history.restore.code.resources_incomplete"]).not.toContain("Choose an available format");
    expect(zhKeys["history.restore.code.resources_incomplete"]).not.toContain("选择可用格式");
    expect(enKeys["history.restore.code.resources_incomplete"]).toMatch(/text/i);
    expect(zhKeys["history.restore.code.resources_incomplete"]).toContain("文字");
    expect(enKeys["history.composite.copy.text"]).toBe("Plain text only");
    expect(enKeys["history.composite.copy.png"]).toBe("Image only");
    expect(zhKeys["history.composite.copy.text"]).toBe("仅复制文字");
    expect(zhKeys["history.composite.copy.png"]).toBe("仅复制图片");
  });

  it("shows shipped wording instead of a raw withheld code", () => {
    const subject = composite({
      formats: ["text", "html", "png"],
      restorable_formats: ["text", "html"],
      withheld_formats: [{ format: "png", code: "no_single_image" }],
    });
    const fallback: Record<string, string> = en;
    const dicts: Array<[string, Record<string, string>]> = [["en", en], ["zh-CN", zhCN]];
    for (const [language, dict] of dicts) {
      const translate = (key: string): string => dict[key] ?? fallback[key] ?? key;
      const withheld =
        rowDiagnostics(subject, translate).find(
          (row) => row.label === translate("history.composite.diag.withheld"),
        )?.value ?? "";
      expect(withheld, language).not.toContain("no_single_image");
      expect(withheld, language).toContain(translate("history.restore.code.no_single_image"));
    }
  });

  it("resolves completeness to shipped wording, not the raw contract value", () => {
    const subject = composite({ completeness: "external_references" });
    const fallback: Record<string, string> = en;
    const dicts: Array<[string, Record<string, string>]> = [["en", en], ["zh-CN", zhCN]];
    for (const [language, dict] of dicts) {
      const translate = (key: string): string => dict[key] ?? fallback[key] ?? key;
      const completeness =
        rowDiagnostics(subject, translate).find(
          (row) => row.label === translate("history.composite.diag.completeness"),
        )?.value ?? "";
      expect(completeness, language).toBe(translate("history.composite.completeness.external_references"));
      expect(completeness, language).not.toContain("external_references");
    }
  });
});

describe("history row details styles", () => {
  function historyRules(): CSSStyleRule[] {
    const style = document.createElement("style");
    style.textContent = readHistoryCss();
    document.head.appendChild(style);
    return Array.from(style.sheet?.cssRules ?? []).filter(
      (rule): rule is CSSStyleRule => rule instanceof CSSStyleRule,
    );
  }

  function readHistoryCss(): string {
    const candidates = [
      resolve(process.cwd(), "src/styles/history.css"),
      resolve(process.cwd(), "windows/src/styles/history.css"),
    ];
    for (const candidate of candidates) {
      if (existsSync(candidate)) return readFileSync(candidate, "utf8");
    }
    throw new Error(`history.css not found from ${process.cwd()}`);
  }

  it("styles the diagnostics panel with theme variables so both schemes resolve", () => {
    const rules = historyRules();
    const panel = rules.find((rule) => rule.selectorText.includes(".history-row-popover"));
    expect(panel).toBeDefined();
    expect(panel?.style.background).toContain("var(--bg-raised)");
    const label = rules.find((rule) => rule.selectorText === ".item-diagnostics dt");
    expect(label?.style.color).toBe("var(--text-tertiary)");
  });
});
