/// <reference types="node" />

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const settingsCssPath = resolve(process.cwd(), "src/styles/settings.css");

function settingsRules(): CSSStyleRule[] {
  const style = document.createElement("style");
  style.textContent = readFileSync(settingsCssPath, "utf8");
  document.head.appendChild(style);

  return Array.from(style.sheet?.cssRules ?? []).filter(
    (rule): rule is CSSStyleRule => rule instanceof CSSStyleRule,
  );
}

describe("theme import dialog layout", () => {
  it("constrains oversized previews and keeps them vertically scrollable", () => {
    const rule = settingsRules().find((candidate) =>
      candidate.selectorText
        .split(",")
        .some((selector) => selector.trim() === ".theme-import-dialog"),
    );

    expect(rule).toBeDefined();
    expect(rule?.style.maxHeight).toContain("100dvh");
    expect(rule?.style.overflowY).toBe("auto");
    expect(rule?.style.scrollbarGutter).toBe("stable");
  });
});
