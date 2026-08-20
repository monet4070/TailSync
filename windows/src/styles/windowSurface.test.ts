/// <reference types="node" />

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function rulesFrom(relativePath: string): CSSStyleRule[] {
  const style = document.createElement("style");
  style.textContent = readFileSync(resolve(process.cwd(), relativePath), "utf8");
  document.head.appendChild(style);

  return Array.from(style.sheet?.cssRules ?? []).filter(
    (rule): rule is CSSStyleRule => rule instanceof CSSStyleRule,
  );
}

describe("transparent window backing surface", () => {
  it("leaves the history/settings document outside .app transparent", () => {
    const rule = rulesFrom("src/styles/history.css").find((candidate) =>
      candidate.selectorText === "html, body",
    );

    expect(rule).toBeDefined();
    expect(rule?.style.background).toBe("transparent");
  });

  it("leaves the preview document outside its surface transparent", () => {
    const rule = rulesFrom("src/styles/preview.css").find((candidate) =>
      candidate.selectorText
        .split(",")
        .map((selector) => selector.trim())
        .join(",") === "html,body,#root",
    );

    expect(rule).toBeDefined();
    expect(rule?.style.background).toBe("transparent");
  });
});
