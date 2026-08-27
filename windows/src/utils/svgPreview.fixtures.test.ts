import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  buildSVGDocument,
  isSVGVisualEligible,
  isTrustEligibleURL,
  summarizeExternalReferences,
} from "./svgPreview";

/**
 * Shared SVG preview policy fixtures: the same JSON drives the Windows
 * (vitest) and macOS (XCTest) suites, so the two implementations of the
 * trust gate, reference extractor, visual eligibility gate, and CSP
 * construction cannot drift apart silently.  `import.meta.url` is rewritten
 * by the test transform, so locate the repository root by walking up from
 * the working directory instead.
 */
function findFixturePath(): string {
  let directory = process.cwd();
  for (let depth = 0; depth < 6; depth += 1) {
    const candidate = join(directory, "shared", "svg-preview-policy-fixtures.json");
    if (existsSync(candidate)) return candidate;
    directory = dirname(directory);
  }
  throw new Error("shared/svg-preview-policy-fixtures.json not found above the working directory");
}

const fixtures = JSON.parse(readFileSync(findFixturePath(), "utf8")) as {
  trustEligibility: { url: string; eligible: boolean }[];
  referenceExtraction: {
    source: string;
    allowedHosts: string[];
    allowedOrigins: string[];
    rejectedHosts: string[];
  }[];
  visualEligibility: { source: string; eligible: boolean }[];
  csp: { source: string; trusted: boolean; expectedCSP: string }[];
};

function cspFromDocument(html: string): string {
  const marker = 'Content-Security-Policy" content="';
  const start = html.indexOf(marker);
  if (start === -1) throw new Error("document has no CSP header");
  const rest = html.slice(start + marker.length);
  return rest.slice(0, rest.indexOf('"'));
}

describe("shared SVG preview policy fixtures", () => {
  it("trust eligibility matches every fixture entry", () => {
    expect(fixtures.trustEligibility.length).toBeGreaterThanOrEqual(30);
    for (const { url, eligible } of fixtures.trustEligibility) {
      expect(isTrustEligibleURL(new URL(url)), url).toBe(eligible);
    }
  });

  it("reference extraction matches every fixture entry", () => {
    expect(fixtures.referenceExtraction.length).toBeGreaterThanOrEqual(3);
    for (const fixture of fixtures.referenceExtraction) {
      const summary = summarizeExternalReferences(fixture.source);
      expect(summary.allowedHosts, fixture.source).toEqual(fixture.allowedHosts);
      expect(summary.allowedOrigins, fixture.source).toEqual(fixture.allowedOrigins);
      expect(summary.rejectedHosts, fixture.source).toEqual(fixture.rejectedHosts);
    }
  });

  it("visual eligibility matches every fixture entry", () => {
    expect(fixtures.visualEligibility.length).toBeGreaterThanOrEqual(10);
    for (const { source, eligible } of fixtures.visualEligibility) {
      expect(isSVGVisualEligible(source), source).toBe(eligible);
    }
  });

  it("emits byte-identical CSP for every fixture entry", () => {
    expect(fixtures.csp.length).toBeGreaterThanOrEqual(3);
    for (const fixture of fixtures.csp) {
      const policy = cspFromDocument(
        buildSVGDocument(fixture.source, fixture.trusted),
      );
      expect(policy, fixture.source).toBe(fixture.expectedCSP);
    }
  });
});
