/// <reference types="node" />

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const themeCssPath = resolve(process.cwd(), "src/styles/theme.css");

function themeRules(): CSSStyleRule[] {
  const style = document.createElement("style");
  style.textContent = readFileSync(themeCssPath, "utf8");
  document.head.appendChild(style);

  return Array.from(style.sheet?.cssRules ?? []).filter(
    (rule): rule is CSSStyleRule => rule instanceof CSSStyleRule,
  );
}

describe("theme component layout", () => {
  it("uses the resolved panel padding to inset card content", () => {
    const rule = themeRules().find((candidate) => candidate.selectorText === ".app .setting-group");

    expect(rule).toBeDefined();
    expect(rule?.style.paddingInline).toBe("var(--theme-panel-default-padding, 0px)");
  });
});
