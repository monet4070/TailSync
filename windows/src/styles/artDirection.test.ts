/// <reference types="node" />

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const artDirectionCssPath = resolve(
  process.cwd(),
  "../shared/art-direction.css",
);

function topLevelRules(): CSSStyleRule[] {
  const style = document.createElement("style");
  style.textContent = readFileSync(artDirectionCssPath, "utf8");
  document.head.appendChild(style);

  return Array.from(style.sheet?.cssRules ?? []).filter(
    (rule): rule is CSSStyleRule => rule instanceof CSSStyleRule,
  );
}

describe("art direction compositing", () => {
  it("keeps app roots off transformed compositor layers", () => {
    const appRootRules = topLevelRules().filter((rule) =>
      rule.selectorText
        .split(",")
        .some((selector) => /^\.app(?:\.[\w-]+)*$/.test(selector.trim())),
    );

    expect(appRootRules.length).toBeGreaterThan(0);
    for (const rule of appRootRules) {
      expect(rule.style.animation).toBe("");
      expect(rule.style.animationName).toBe("");
      expect(rule.style.transform).toBe("");
      expect(rule.style.filter).toBe("");
      expect(rule.style.opacity).toBe("");
      expect(rule.style.willChange).toBe("");
    }
  });
});
