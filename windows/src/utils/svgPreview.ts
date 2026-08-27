/**
 * Safety and presentation policy shared by the Windows SVG preview.
 *
 * SVG is displayed inside a sandboxed WebView2 iframe whose srcdoc carries a
 * restrictive CSP.  The extractor is intentionally not treated as the
 * security boundary: trusted CSP sources are an exact allow-list, so an
 * unrecognised reference can only remain blocked, never widen access.
 */

export const SVG_PREVIEW_MAX_BYTES = 8 * 1024 * 1024;
export const SVG_PREVIEW_MAX_DIMENSION = 4_096;
export const SVG_PREVIEW_MAX_PIXELS = 16 * 1024 * 1024;
export const SVG_PREVIEW_TIMEOUT_MS = 4_000;
export const SVG_PREVIEW_DEFAULT_VIEWPORT = { width: 1_024, height: 768 } as const;

const NON_VISUAL_MARKUP = /<(?:animate|animateMotion|animateTransform|base|embed|form|frame|iframe|link|meta|object|script|set)\b|@(?:-webkit-)?keyframes\b|\banimation(?:-name|-duration|-delay|-iteration-count|-timing-function|-fill-mode|\s*:)/i;

export interface SVGExternalReferenceSummary {
  allowedHosts: string[];
  allowedOrigins: string[];
  rejectedHosts: string[];
}

/**
 * Reject markup that can navigate or create an active browsing context before
 * it reaches the sandbox.  The CSP and iframe sandbox remain the security
 * boundary; this gate only avoids ambiguous browser parsing and gives the
 * user a reliable source-view fallback for active documents.
 */
export function isSVGVisualEligible(source: string): boolean {
  return /<svg\b/i.test(source) && !NON_VISUAL_MARKUP.test(source);
}

const NAMED_ENTITIES: Record<string, string> = {
  amp: "&",
  apos: "'",
  bsol: "\\",
  colon: ":",
  commat: "@",
  equals: "=",
  gt: ">",
  lt: "<",
  num: "#",
  period: ".",
  semi: ";",
  sol: "/",
  quot: '"',
};

function decodeHtmlEntities(value: string): string {
  if (!value.includes("&")) return value;
  return value.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, body: string) => {
    if (body.startsWith("#x") || body.startsWith("#X")) {
      const scalar = Number.parseInt(body.slice(2), 16);
      return Number.isInteger(scalar) && scalar >= 0 && scalar <= 0x10ffff
        ? String.fromCodePoint(scalar)
        : whole;
    }
    if (body.startsWith("#")) {
      const scalar = Number.parseInt(body.slice(1), 10);
      return Number.isInteger(scalar) && scalar >= 0 && scalar <= 0x10ffff
        ? String.fromCodePoint(scalar)
        : whole;
    }
    return NAMED_ENTITIES[body.toLowerCase()] ?? whole;
  });
}

function attributeTargets(source: string): string[] {
  const targets: string[] = [];
  const pattern = /(?:^|[\s<])(href|xlink:href|srcset|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/gi;
  for (const match of source.matchAll(pattern)) {
    const name = match[1].toLowerCase();
    const raw = match[2] ?? match[3] ?? match[4] ?? "";
    if (name === "srcset") {
      for (const candidate of raw.split(",")) {
        const token = candidate.trim().split(/\s+/, 1)[0];
        if (token) targets.push(token);
      }
    } else {
      targets.push(raw);
    }
  }
  return targets;
}

function cssTargets(source: string): string[] {
  const targets: string[] = [];
  const pattern = /url\(\s*(?:'([^']*)'|"([^"]*)"|([^)]*))\s*\)/gi;
  for (const match of source.matchAll(pattern)) {
    targets.push(match[1] ?? match[2] ?? match[3] ?? "");
  }
  return targets;
}

/** Returns distinct absolute HTTP(S) subresource references in source order. */
export function externalReferences(source: string): URL[] {
  const targets = [...attributeTargets(source), ...cssTargets(source)];
  const seen = new Set<string>();
  const references: URL[] = [];
  for (const raw of targets) {
    const target = decodeHtmlEntities(raw).trim();
    let url: URL;
    try {
      url = new URL(target);
    } catch {
      continue;
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") continue;
    if (!url.hostname || seen.has(url.origin)) continue;
    seen.add(url.origin);
    references.push(url);
  }
  return references;
}

function parseIPv4(host: string): number[] | null {
  const parts = host.split(".");
  if (parts.length !== 4 || parts.some((part) => !/^\d+$/.test(part))) return null;
  const octets = parts.map((part) => Number(part));
  return octets.every((octet) => Number.isInteger(octet) && octet >= 0 && octet <= 255)
    ? octets
    : null;
}

function isNonPublicIPv4(octets: number[]): boolean {
  const [a, b, c] = octets;
  if (a === 0 || a === 10 || a === 127) return true;
  if (a === 100 && b >= 64 && b <= 127) return true;
  if (a === 169 && b === 254) return true;
  if (a === 172 && b >= 16 && b <= 31) return true;
  if (a === 192 && b === 168) return true;
  if (a === 192 && b === 0 && (c === 0 || c === 2)) return true;
  if (a === 198 && (b === 18 || b === 19)) return true;
  if (a === 198 && b === 51 && c === 100) return true;
  if (a === 203 && b === 0 && c === 113) return true;
  return a >= 224;
}

function parseIPv6(host: string): number[] | null {
  const normalized = host.replace(/^\[/, "").replace(/\]$/, "").toLowerCase();
  if (normalized.includes("%")) return null;
  const halves = normalized.split("::");
  if (halves.length > 2) return null;

  const parsePart = (part: string): number[] | null => {
    if (part.includes(".")) {
      const ipv4 = parseIPv4(part);
      return ipv4 === null ? null : [
        ipv4[0] * 256 + ipv4[1],
        ipv4[2] * 256 + ipv4[3],
      ];
    }
    if (!/^[0-9a-f]{1,4}$/.test(part)) return null;
    return [Number.parseInt(part, 16)];
  };

  const parseHalf = (half: string): number[] | null => {
    if (!half) return [];
    const result: number[] = [];
    for (const part of half.split(":")) {
      const parsed = parsePart(part);
      if (parsed === null) return null;
      result.push(...parsed);
    }
    return result;
  };

  const left = parseHalf(halves[0]);
  const right = halves.length === 2 ? parseHalf(halves[1]) : [];
  if (left === null || right === null) return null;
  const groups = halves.length === 2
    ? [...left, ...Array(8 - left.length - right.length).fill(0), ...right]
    : left;
  if (groups.length !== 8) return null;
  const bytes: number[] = [];
  for (const group of groups) bytes.push(group >> 8, group & 0xff);
  return bytes;
}

function isNonPublicLiteralHost(host: string): boolean {
  const normalized = host.toLowerCase().replace(/^\[/, "").replace(/\]$/, "");
  if (
    normalized === "localhost" ||
    normalized.endsWith(".localhost") ||
    normalized.endsWith(".local")
  ) return true;

  const ipv4 = parseIPv4(normalized);
  if (ipv4 !== null) return isNonPublicIPv4(ipv4);

  if (!normalized.includes(":")) return false;
  const ipv6 = parseIPv6(normalized);
  // A malformed/scoped IPv6 literal is safer to reject than to disclose as a
  // public host. URL normally rejects these before this function is reached.
  if (ipv6 === null) return true;
  if (ipv6.every((byte) => byte === 0)) return true;
  if (ipv6.slice(0, 15).every((byte) => byte === 0) && ipv6[15] === 1) return true;
  if ((ipv6[0] & 0xfe) === 0xfc) return true; // fc00::/7 unique-local
  if (ipv6[0] === 0xfe && (ipv6[1] & 0xc0) === 0x80) return true; // fe80::/10
  if (ipv6[0] === 0xff) return true; // multicast
  if (ipv6[0] === 0x20 && ipv6[1] === 0x01 && ipv6[2] === 0x0d && ipv6[3] === 0xb8) return true; // 2001:db8::/32
  if (ipv6.slice(0, 10).every((byte) => byte === 0) && ipv6[10] === 0xff && ipv6[11] === 0xff) {
    return isNonPublicIPv4([
      ipv6[12], ipv6[13], ipv6[14], ipv6[15],
    ]);
  }
  return false;
}

/** Trust requires HTTPS, no embedded credentials, and a public literal host. */
export function isTrustEligibleURL(url: URL): boolean {
  return url.protocol === "https:" &&
    url.username === "" &&
    url.password === "" &&
    !isNonPublicLiteralHost(url.hostname);
}

export function summarizeExternalReferences(source: string): SVGExternalReferenceSummary {
  const summary: SVGExternalReferenceSummary = {
    allowedHosts: [],
    allowedOrigins: [],
    rejectedHosts: [],
  };
  for (const url of externalReferences(source)) {
    const host = url.host.toLowerCase();
    if (isTrustEligibleURL(url)) {
      if (!summary.allowedHosts.includes(host)) summary.allowedHosts.push(host);
      if (!summary.allowedOrigins.includes(url.origin)) summary.allowedOrigins.push(url.origin);
    } else if (!summary.rejectedHosts.includes(host)) {
      summary.rejectedHosts.push(host);
    }
  }
  return summary;
}

function cspPolicy(source: string, trustingExternalResources: boolean): string {
  const summary = trustingExternalResources
    ? summarizeExternalReferences(source)
    : { allowedOrigins: [] as string[] };
  const origins = summary.allowedOrigins.join(" ");
  const imageSources = origins ? `data: ${origins}` : "data:";
  return [
    "default-src 'none'",
    "style-src 'unsafe-inline'",
    `img-src ${imageSources}`,
    `font-src ${imageSources}`,
    "media-src 'none'",
    "object-src 'none'",
    "frame-src 'none'",
    "child-src 'none'",
    "worker-src 'none'",
    "connect-src 'none'",
    "manifest-src 'none'",
    "script-src 'none'",
    "navigate-to 'none'",
    "base-uri 'none'",
    "form-action 'none'",
  ].join("; ");
}

/** Host document for a sandboxed iframe. It never uses a Blob or filesystem URL. */
export function buildSVGDocument(source: string, trustingExternalResources: boolean): string {
  const policy = cspPolicy(source, trustingExternalResources).replaceAll('"', "&quot;");
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${policy}"><style>html,body{width:100%;height:100%;margin:0;overflow:hidden;background:transparent}body{display:grid;place-items:center}.tailsync-svg-root{display:grid;place-items:center;width:100%;height:100%;overflow:hidden}.tailsync-svg-root>svg{display:block;max-width:100%;max-height:100%}</style></head><body><div class="tailsync-svg-root">${source}</div></body></html>`;
}

function cssLength(value: string): number | null {
  const match = value.trim().match(/^([+-]?(?:\d+(?:\.\d*)?|\.\d+))(px|pt|pc|in|cm|mm|q)?$/i);
  if (!match) return null;
  const number = Number(match[1]);
  if (!Number.isFinite(number) || number <= 0) return null;
  switch ((match[2] ?? "px").toLowerCase()) {
    case "px": return number;
    case "pt": return number * 96 / 72;
    case "pc": return number * 16;
    case "in": return number * 96;
    case "cm": return number * 96 / 2.54;
    case "mm": return number * 96 / 25.4;
    case "q": return number * 96 / 101.6;
    default: return null;
  }
}

function clampViewport(width: number, height: number): { width: number; height: number } {
  let scale = Math.min(1, SVG_PREVIEW_MAX_DIMENSION / width, SVG_PREVIEW_MAX_DIMENSION / height);
  const pixels = width * height;
  if (pixels > SVG_PREVIEW_MAX_PIXELS) scale = Math.min(scale, Math.sqrt(SVG_PREVIEW_MAX_PIXELS / pixels));
  return { width: Math.max(1, width * scale), height: Math.max(1, height * scale) };
}

/** Reads the root SVG intrinsic size and applies the same output budgets as macOS. */
export function svgViewport(source: string): { width: number; height: number } {
  const tag = source.match(/<svg\b[^>]*>/i)?.[0];
  if (!tag) return { ...SVG_PREVIEW_DEFAULT_VIEWPORT };
  const read = (name: string): number | null => {
    const value = tag.match(new RegExp(`(?:^|\\s)${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`, "i"));
    return cssLength(value?.[1] ?? value?.[2] ?? value?.[3] ?? "");
  };
  const width = read("width");
  const height = read("height");
  return width !== null && height !== null ? clampViewport(width, height) : { ...SVG_PREVIEW_DEFAULT_VIEWPORT };
}
