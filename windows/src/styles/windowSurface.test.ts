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

describe("favorite history-row surface", () => {
  it("keeps the completed long-press fill visible for favorite rows", () => {
    const rules = rulesFrom("src/styles/history.css");
    const favorite = rules.find((candidate) =>
      candidate.selectorText === ".history-item.is-favorite .favorite-press-progress",
    );
    const progress = rules.find((candidate) =>
      candidate.selectorText === ".favorite-press-progress",
    );

    expect(favorite).toBeDefined();
    expect(favorite?.style.transform).toBe("scaleX(1)");
    expect(favorite?.style.opacity).toBe("1");
    expect(progress?.style.transition).toContain("transform 0.42s linear");
  });

  it("keeps the favorite stamp in the metadata flow instead of overlaying text", () => {
    const stamp = rulesFrom("src/styles/history.css").find((candidate) =>
      candidate.selectorText === ".favorite-stamp",
    );

    expect(stamp).toBeDefined();
    expect(stamp?.style.position).toBe("static");
    expect(stamp?.style.flex).toBe("0 0 18px");
  });
});

describe("focused history-row surface", () => {
  it("keeps selection on the focus semantic and gives it a visible frame", () => {
    const historyRules = rulesFrom("src/styles/history.css");
    const utilityRules = rulesFrom("src/styles/utilities.css");
    const focused = historyRules.find((candidate) =>
      candidate.selectorText === ".history-item.focused",
    );
    const selected = utilityRules.find((candidate) =>
      candidate.selectorText === '.app .history-item[aria-selected="true"]',
    );

    expect(focused).toBeDefined();
    expect(focused?.style.background).toContain("--theme-history-focus-background");
    expect(focused?.style.boxShadow).toContain("--theme-history-focus-border");
    expect(selected).toBeDefined();
    expect(selected?.style.background).toContain("--theme-history-focus-background");
    expect(selected?.style.color).toContain("--theme-history-focus-foreground");
  });
});
