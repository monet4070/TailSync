export type CssPair = readonly [string, string];

export interface ThemeV2CssOptions {
  reduceTransparency?: boolean;
  mode?: "light" | "dark";
  /** The persisted V2 colour-theme id, used for semantic history typography. */
  themeId?: string;
}

export type HistoryFontRole = "display" | "reading";
type JsonObject = Record<string, unknown>;

/**
 * Keep the history-row font selection aligned with macOS HistoryRow.
 * Canvas/Ledger and custom themes use their expressive display face; the
 * denser Flux/Aura/Mono themes keep history content in the reading face.
 * Unknown ids fall back to the default Canvas behavior.
 */
export function historyFontRole(themeId?: string): HistoryFontRole {
  return themeId === "builtin:flux@1"
    || themeId === "builtin:aura@1"
    || themeId === "builtin:mono@1"
    ? "reading"
    : "display";
}

interface ParsedColor { red: number; green: number; blue: number; alpha: number }

const parseColor = (token: string): ParsedColor | undefined => {
  const hex = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(token);
  if (hex) {
    const value = Number.parseInt(hex[1], 16);
    return {
      red: (value >> 16) & 0xff,
      green: (value >> 8) & 0xff,
      blue: value & 0xff,
      alpha: hex[2] ? Number.parseInt(hex[2], 16) / 255 : 1,
    };
  }
  const rgba = /^rgba\(\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)\s*,\s*(\d*\.?\d+)\s*\)$/i.exec(token);
  if (!rgba) return undefined;
  const [red, green, blue, alpha] = rgba.slice(1).map(Number);
  if ([red, green, blue, alpha].some(Number.isNaN)
    || [red, green, blue].some(channel => channel < 0 || channel > 255)
    || alpha < 0 || alpha > 1) return undefined;
  return { red, green, blue, alpha };
};

const composite = (color: ParsedColor, backdrop: ParsedColor): ParsedColor => ({
  red: Math.round(color.red * color.alpha + backdrop.red * (1 - color.alpha)),
  green: Math.round(color.green * color.alpha + backdrop.green * (1 - color.alpha)),
  blue: Math.round(color.blue * color.alpha + backdrop.blue * (1 - color.alpha)),
  alpha: 1,
});

const opaqueCssColor = (token: string, backdrop: ParsedColor): string => {
  const parsed = parseColor(token);
  if (!parsed) return token === "system" ? "AccentColor" : token;
  const opaque = parsed.alpha < 1 ? composite(parsed, backdrop) : parsed;
  return `rgb(${Math.round(opaque.red)}, ${Math.round(opaque.green)}, ${Math.round(opaque.blue)})`;
};

const isJsonObject = (candidate: unknown): candidate is JsonObject =>
  candidate !== null && typeof candidate === "object" && !Array.isArray(candidate);
const value = (root: JsonObject | undefined, path: string[]): unknown =>
  path.reduce<unknown>((current, key) => isJsonObject(current) ? current[key] : undefined, root);
const color = (tokens: JsonObject, path: string[]) => value(tokens, path);
const cssColor = (token: string): string => token === "system" ? "AccentColor" : token;
const componentNames = ["search", "history", "section", "panel", "button", "input", "toast"] as const;
const componentStates = ["default", "hover", "active", "selected", "disabled", "focus"] as const;
const componentColorFields = ["background", "foreground", "secondaryText", "border", "focusRing", "icon", "accent"] as const;

const componentProperty = (component: string, state: string, field: string) =>
  `--theme-${component}-${state}-${field.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`)}`;

const appendComponentPairs = (
  pairs: CssPair[],
  tokens: JsonObject,
  mapColor: (token: string) => string,
  removeShadows: boolean,
) => {
  for (const component of componentNames) for (const state of componentStates) {
    const root = value(tokens, ["components", component, state]);
    if (!isJsonObject(root)) continue;
    for (const field of componentColorFields) {
      if (typeof root[field] === "string") pairs.push([componentProperty(component, state, field), mapColor(root[field])]);
    }
    for (const field of ["radius", "padding", "spacing"] as const) {
      if (typeof root[field] === "number") pairs.push([componentProperty(component, state, field), `${root[field]}px`]);
    }
    if (isJsonObject(root.typography)) {
      if (typeof root.typography.size === "number") pairs.push([componentProperty(component, state, "font-size"), `${root.typography.size}px`]);
      if (typeof root.typography.weight === "number") pairs.push([componentProperty(component, state, "font-weight"), String(root.typography.weight)]);
    }
    if (isJsonObject(root.shadow)) {
      const { radius, y, opacity } = root.shadow;
      if (typeof radius === "number" && typeof y === "number" && typeof opacity === "number") {
        pairs.push([componentProperty(component, state, "shadow"), removeShadows ? "none" : `0 ${y}px ${radius}px rgba(0, 0, 0, ${opacity})`]);
      }
    }
  }
};

/** Converts the fully resolved, data-only V2 token tree to the app's CSS contract. */
export function themeV2CssPairs(tokens: JsonObject, options: ThemeV2CssOptions = {}): CssPair[] {
  const fallback = options.mode === "dark"
    ? { red: 0, green: 0, blue: 0, alpha: 1 }
    : { red: 255, green: 255, blue: 255, alpha: 1 };
  const canvasToken = value(tokens, ["colors", "background", "canvas"]);
  const canvas = typeof canvasToken === "string" ? parseColor(canvasToken) ?? fallback : fallback;
  const opaqueCanvas = canvas.alpha < 1 ? composite(canvas, fallback) : canvas;
  const mapColor = options.reduceTransparency
    ? (token: string) => opaqueCssColor(token, opaqueCanvas)
    : cssColor;
  const colors: [string, string[]][] = [
    ["--brand", ["colors","accent","default"]], ["--brand-hover", ["colors","accent","hover"]], ["--brand-soft", ["colors","accent","soft"]], ["--brand-text", ["colors","accent","onAccent"]],
    ["--bg-window", ["colors","background","canvas"]], ["--bg-card", ["colors","background","surface"]], ["--bg-input", ["colors","background","input"]], ["--bg-hover", ["colors","background","hover"]], ["--bg-active", ["colors","background","active"]], ["--bg-raised", ["colors","background","raised"]], ["--bg-toast", ["colors","background","toast"]],
    ["--text-primary", ["colors","text","primary"]], ["--text-secondary", ["colors","text","secondary"]], ["--text-tertiary", ["colors","text","tertiary"]], ["--text-toast", ["colors","text","toast"]],
    ["--border", ["colors","border","default"]], ["--border-strong", ["colors","border","strong"]], ["--divider", ["colors","border","divider"]],
    ["--green", ["colors","status","positive"]], ["--green-soft", ["colors","status","positiveSoft"]], ["--orange", ["colors","status","warning"]], ["--orange-soft", ["colors","status","warningSoft"]], ["--purple", ["colors","status","info"]], ["--purple-soft", ["colors","status","infoSoft"]],
  ];
  const pairs: CssPair[] = colors.flatMap(([name, path]) => { const token = color(tokens, path); return typeof token === "string" ? [[name, mapColor(token)] as CssPair] : []; });
  const number = (name: string, path: string[], suffix = "px") => { const n = value(tokens, path); if (typeof n === "number") pairs.push([name, `${n}${suffix}`]); };
  const families = (name: string, path: string[]) => { const x=value(tokens,path); if (Array.isArray(x) && x.every(v => typeof v === "string")) pairs.push([name, x.join(", ")]); };
  families("--font-ui", ["typography","ui","families"]); families("--font-display", ["typography","display","families"]); families("--font-content", ["typography","reading","families"]);
  pairs.push(["--font-history", historyFontRole(options.themeId) === "display" ? "var(--font-display)" : "var(--font-content)"]);
  number("--font-size-ui", ["typography","ui","size"]); number("--line-height-ui", ["typography","ui","lineHeight"], "px"); number("--font-weight-body", ["typography","ui","weight"], "");
  number("--search-font-size", ["typography","search","size"]); number("--font-size-section", ["typography","section","size"]); number("--font-size-content", ["typography","history","size"]);
  if (typeof value(tokens,["typography","search","useDisplayFont"]) === "boolean") pairs.push(["--search-font-family", value(tokens,["typography","search","useDisplayFont"]) ? "var(--font-display)" : "var(--font-ui)"]);
  if (typeof value(tokens,["typography","section","uppercase"]) === "boolean") pairs.push(["--section-title-transform", value(tokens,["typography","section","uppercase"]) ? "uppercase" : "none"]);
  number("--radius-sm", ["shape","controlRadius"]); number("--radius-md", ["shape","surfaceRadius"]); number("--window-radius", ["shape","windowRadius"]); number("--history-row-padding-y", ["density","row"]); number("--setting-row-padding-y", ["density","control"]);
  const radius=value(tokens,["effects","shadow","radius"]), y=value(tokens,["effects","shadow","y"]), opacity=value(tokens,["effects","shadow","opacity"]); if (typeof radius === "number" && typeof y === "number" && typeof opacity === "number") pairs.push(["--shadow-md", options.reduceTransparency ? "none" : `0 ${y}px ${radius}px rgba(0, 0, 0, ${opacity})`]);
  number("--motion-fast", ["effects","motion","fast"], "ms"); number("--motion-slow", ["effects","motion","slow"], "ms");
  number("--art-fast", ["effects","motion","fast"], "ms"); number("--art-slow", ["effects","motion","slow"], "ms");
  const easing=value(tokens,["effects","motion","easing"]); if (typeof easing === "string") pairs.push(["--motion-easing", easing], ["--art-spring", easing]);
  appendComponentPairs(pairs, tokens, mapColor, Boolean(options.reduceTransparency));
  return pairs;
}

export const themeV2CssProperties = [
  "--brand","--brand-hover","--brand-soft","--brand-text","--bg-window","--bg-card","--bg-input","--bg-hover","--bg-active","--bg-raised","--bg-toast","--text-primary","--text-secondary","--text-tertiary","--text-toast","--border","--border-strong","--divider","--green","--green-soft","--orange","--orange-soft","--purple","--purple-soft","--font-ui","--font-display","--font-content","--font-history","--font-size-ui","--line-height-ui","--font-weight-body","--search-font-size","--font-size-section","--font-size-content","--search-font-family","--section-title-transform","--radius-sm","--radius-md","--window-radius","--history-row-padding-y","--setting-row-padding-y","--shadow-md","--motion-fast","--motion-slow","--motion-easing","--art-fast","--art-slow","--art-spring","--theme-logo-image","--theme-empty-state-image","--theme-preview-placeholder-image",
  ...componentNames.flatMap(component => componentStates.flatMap(state => [
    ...componentColorFields.map(field => componentProperty(component, state, field)),
    componentProperty(component, state, "radius"), componentProperty(component, state, "padding"), componentProperty(component, state, "spacing"),
    componentProperty(component, state, "font-size"), componentProperty(component, state, "font-weight"), componentProperty(component, state, "shadow"),
  ])),
];
