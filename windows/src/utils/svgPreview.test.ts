import { describe, expect, it } from "vitest";
import {
  buildSVGDocument,
  externalReferences,
  isTrustEligibleURL,
  isSVGVisualEligible,
  summarizeExternalReferences,
} from "./svgPreview";

describe("SVG preview document policy", () => {
  it("creates a sandbox-ready document with network and active content blocked by default", () => {
    const source = `<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script><image href="https://cdn.example.com/a.png"/></svg>`;
    const html = buildSVGDocument(source, false);

    expect(html).toContain("default-src 'none'");
    expect(html).toContain("script-src 'none'");
    expect(html).toContain("connect-src 'none'");
    expect(html).toContain("frame-src 'none'");
    expect(html).toContain("navigate-to 'none'");
    expect(html).toContain("base-uri 'none'");
    expect(html).toContain("form-action 'none'");
    expect(html).toContain("img-src data:");
    expect(html).toContain("font-src data:");
    expect(html).not.toContain("img-src data: https://cdn.example.com");
    expect(html).not.toContain("img-src https:");
    expect(html).not.toContain("font-src https:");
    expect(html).toContain(source);
  });

  it("discloses srcset, entity-encoded and CSS references with exact origins", () => {
    const source = `<svg><image srcset="https://localhost:9443/a.png 1x, https://cdn.example.com:8443/a.png 2x" href="https&#58;//localhost:9443/b.png"/><style>.x{background:url('https&#58;//cdn.example.com:8443/c.png')}</style></svg>`;
    const summary = summarizeExternalReferences(source);

    expect(summary.allowedHosts).toEqual(["cdn.example.com:8443"]);
    expect(summary.allowedOrigins).toEqual(["https://cdn.example.com:8443"]);
    expect(summary.rejectedHosts).toEqual(["localhost:9443"]);
    expect(externalReferences(source).map((url) => url.host)).toEqual([
      "localhost:9443",
      "cdn.example.com:8443",
    ]);
  });

  it("refuses browser-compatible private IPv4 spellings and local IPv6 ranges", () => {
    for (const target of [
      "https://2130706433/a.png",
      "https://0x7f000001/a.png",
      "https://0177.0.0.1/a.png",
      "https://127.1/a.png",
      "https://127.0.1/a.png",
      "https://[::1]/a.png",
      "https://[fd00::1]/a.png",
      "https://[fe80::1]/a.png",
      "https://[2001:db8::1]/a.png",
      "https://[::ffff:127.0.0.1]/a.png",
    ]) {
      expect(isTrustEligibleURL(new URL(target))).toBe(false);
    }
    expect(isTrustEligibleURL(new URL("https://cdn.example.com/a.png"))).toBe(true);
  });

  it("allows only disclosed origins in trusted mode and preserves ports", () => {
    const source = `<svg><image href="https://cdn.example.com:8443/a.png"/><image href="http://plain.example.com/b.png"/></svg>`;
    const html = buildSVGDocument(source, true);

    expect(html).toContain("img-src data: https://cdn.example.com:8443");
    expect(html).toContain("font-src data: https://cdn.example.com:8443");
    expect(html).not.toContain("img-src data: https://cdn.example.com ");
    expect(html).not.toContain("https://plain.example.com");
    expect(html).toContain(source);
  });

  it("routes active or navigation markup to the source fallback", () => {
    expect(isSVGVisualEligible("<svg><script>window.location='https://example.com'</script></svg>")).toBe(false);
    expect(isSVGVisualEligible("<svg><meta http-equiv='refresh' content='0;url=https://example.com'/></svg>")).toBe(false);
    expect(isSVGVisualEligible("<svg><animate attributeName='x' from='0' to='1'/></svg>")).toBe(false);
    expect(isSVGVisualEligible("<svg><foreignObject><div>safe</div></foreignObject></svg>")).toBe(true);
  });
});
