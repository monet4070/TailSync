// Pure helpers that turn a validated ThemeEntry into CSS custom properties
// for inline injection (R006). Nothing here touches the DOM; the hook in
// useTheme applies the pairs with style.setProperty().
//
// Token order and naming follow THEMING.md §2.2 / shared/art-direction.css
// theme blocks. Opacity-bearing colours are emitted as rgba() strings the
// same way the built-in blocks bake them.

import type {
  ThemeBackgroundPayload,
  ThemeColorSpec,
  ThemeEntry,
  ThemeMetrics,
  ThemePalette,
  ThemeTypography,
} from "../tailsyncClient";

/** The 24 palette tokens in contract order: (palette field, CSS var name). */
export const PALETTE_TOKEN_PAIRS: ReadonlyArray<readonly [keyof ThemePalette, string]> = [
  ["brand", "--brand"],
  ["brandHover", "--brand-hover"],
  ["brandSoft", "--brand-soft"],
  ["brandText", "--brand-text"],
  ["bgWindow", "--bg-window"],
  ["bgCard", "--bg-card"],
  ["bgInput", "--bg-input"],
  ["bgHover", "--bg-hover"],
  ["bgActive", "--bg-active"],
  ["bgRaised", "--bg-raised"],
  ["bgToast", "--bg-toast"],
  ["textPrimary", "--text-primary"],
  ["textSecondary", "--text-secondary"],
  ["textTertiary", "--text-tertiary"],
  ["textToast", "--text-toast"],
  ["border", "--border"],
  ["borderStrong", "--border-strong"],
  ["divider", "--divider"],
  ["green", "--green"],
  ["greenSoft", "--green-soft"],
  ["orange", "--orange"],
  ["orangeSoft", "--orange-soft"],
  ["purple", "--purple"],
  ["purpleSoft", "--purple-soft"],
];

/** Font variables injected from ThemeFonts (only when a name is present). */
export const FONT_VARIABLE_PAIRS: ReadonlyArray<readonly ["display" | "reading", string]> = [
  ["display", "--art-display"],
  ["reading", "--art-reading"],
];

/** Structural overrides applied as element style properties (F12 semantics). */
export const STRUCTURAL_PROPERTIES = {
  borderRadius: "border-radius",
  shadow: "box-shadow",
} as const;

/**
 * R007: metrics/typography field → CSS variable mapping. Deterministic
 * one-to-one correspondence documented in THEMING.md §3.6; values are the
 * raw numbers from the theme (px / text-transform / font stack), injected
 * only via style.setProperty. `rowPadding` drives both row paddings and
 * `cardRadius` drives both card and window-surface radii.
 */
export const METRICS_VARIABLE_PAIRS: ReadonlyArray<readonly [keyof ThemeMetrics, string]> = [
  ["controlRadius", "--radius-sm"],
  ["cardRadius", "--radius-md"],
  ["cardRadius", "--window-radius"],
  ["rowPadding", "--history-row-padding-y"],
  ["rowPadding", "--setting-row-padding-y"],
];

export const TYPOGRAPHY_VARIABLE_PAIRS: ReadonlyArray<
  readonly [keyof ThemeTypography, string]
> = [
  ["sectionTitleSize", "--font-size-section"],
  ["historyContentSize", "--font-size-content"],
  ["searchSize", "--search-font-size"],
  ["searchUsesDisplayFont", "--search-font-family"],
  ["uppercasesSectionTitles", "--section-title-transform"],
];

/** Search input font stack when searchUsesDisplayFont is true/false. */
export const SEARCH_DISPLAY_FONT_STACK = "var(--font-display)";
export const SEARCH_UI_FONT_STACK = "var(--font-ui)";
/** Section-title text transform when uppercasesSectionTitles is true/false. */
export const SECTION_TITLES_UPPERCASE = "uppercase";
export const SECTION_TITLES_AS_IS = "none";

function hexToRgb(hex: string): [number, number, number] | null {
  const match = /^#([0-9a-fA-F]{6})$/.exec(hex);
  if (!match) return null;
  const value = parseInt(match[1], 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

/** Hex as-is, or rgba(r, g, b, opacity) when an opacity is present. */
export function colorSpecCssValue(hex: string, opacity?: number | null): string {
  if (opacity === undefined || opacity === null) return hex;
  const rgb = hexToRgb(hex);
  if (!rgb) return hex;
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${opacity})`;
}

/**
 * R007: metrics → style pairs. `shadowRadius` becomes the `--shadow-md`
 * value (the card/floating-surface shadow): a deterministic shadow with
 * the theme's blur radius, or `none` when the radius is 0 (matching the
 * macOS rule `shadowRadius == 0 → no shadow`).
 */
export function metricsCssPairs(metrics: ThemeMetrics): Array<[string, string]> {
  const pairs: Array<[string, string]> = METRICS_VARIABLE_PAIRS.map(([field, name]) => [
    name,
    `${metrics[field]}px`,
  ]);
  pairs.push([
    "--shadow-md",
    metrics.shadowRadius > 0
      ? `0 4px ${metrics.shadowRadius}px rgba(0, 0, 0, 0.08)`
      : "none",
  ]);
  return pairs;
}

/** R007: typography → style pairs (raw numbers and boolean-driven values). */
export function typographyCssPairs(typography: ThemeTypography): Array<[string, string]> {
  return [
    ["--font-size-section", `${typography.sectionTitleSize}px`],
    ["--font-size-content", `${typography.historyContentSize}px`],
    ["--search-font-size", `${typography.searchSize}px`],
    [
      "--search-font-family",
      typography.searchUsesDisplayFont ? SEARCH_DISPLAY_FONT_STACK : SEARCH_UI_FONT_STACK,
    ],
    [
      "--section-title-transform",
      typography.uppercasesSectionTitles ? SECTION_TITLES_UPPERCASE : SECTION_TITLES_AS_IS,
    ],
  ];
}

/**
 * R007: every CSS variable driven by metrics/typography. Exported so tests
 * can assert the exact contract and the hook can clear them on switch.
 */
export function metricsTypographyPropertyNames(): string[] {
  return [
    ...METRICS_VARIABLE_PAIRS.map(([, name]) => name),
    "--shadow-md",
    "--font-size-section",
    "--font-size-content",
    "--search-font-size",
    "--search-font-family",
    "--section-title-transform",
  ];
}

export interface ThemeCssResult {
  /** `[propertyName, value]` pairs: `--x` custom properties plus the
   * structural `border-radius` / `box-shadow` overrides. */
  pairs: Array<[string, string]>;
  /** Unsupported structural keys/values, reported via console.warn. */
  warnings: string[];
}

/**
 * Build the full set of style pairs to inject for a custom theme in the given
 * light/dark mode: the 24 palette tokens, font variables when set, and the
 * V1 structural overrides (borderRadius, shadow: false).
 */
export function buildThemeCss(entry: ThemeEntry, mode: "light" | "dark"): ThemeCssResult {
  const palette = mode === "light" ? entry.palette.light : entry.palette.dark;
  const pairs: Array<[string, string]> = PALETTE_TOKEN_PAIRS.map(([field, name]) => [
    name,
    colorSpecCssValue(palette[field].hex, palette[field].opacity),
  ]);
  const warnings: string[] = [];

  for (const [field, name] of FONT_VARIABLE_PAIRS) {
    const font = entry.fonts[field];
    if (font) pairs.push([name, font]);
  }

  // R007: metrics/typography drive the design-system variables.
  pairs.push(...metricsCssPairs(entry.metrics));
  pairs.push(...typographyCssPairs(entry.typography));

  const structural = entry.structural;
  if (structural) {
    for (const key of Object.keys(structural)) {
      if (key === "borderRadius" || key === "shadow") continue;
      warnings.push(`Ignoring unsupported structural key "${key}" in theme "${entry.id}"`);
    }
    if (structural.borderRadius !== undefined && structural.borderRadius !== null) {
      pairs.push([STRUCTURAL_PROPERTIES.borderRadius, `${structural.borderRadius}px`]);
    }
    if (structural.shadow === false) {
      pairs.push([STRUCTURAL_PROPERTIES.shadow, "none"]);
    } else if (structural.shadow === true) {
      warnings.push(`Ignoring unsupported structural value "shadow: true" in theme "${entry.id}"`);
    }
  }

  return { pairs, warnings };
}

/** Every property name that custom-theme injection may set, so switching back
 * to a built-in theme can remove exactly those (no residual overrides). */
export function allCustomThemeProperties(): string[] {
  return [
    ...PALETTE_TOKEN_PAIRS.map(([, name]) => name),
    ...FONT_VARIABLE_PAIRS.map(([, name]) => name),
    ...metricsTypographyPropertyNames(),
    STRUCTURAL_PROPERTIES.borderRadius,
    STRUCTURAL_PROPERTIES.shadow,
    BACKGROUND_PROPERTY_IMAGE,
    BACKGROUND_PROPERTY_SCRIM,
  ];
}

/** CSS variables consumed by the layered `.app` background rule in
 * shared/art-direction.css (image layer + same-colour scrim gradient). */
export const BACKGROUND_PROPERTY_IMAGE = "--art-bg-image";
export const BACKGROUND_PROPERTY_SCRIM = "--art-bg-scrim";

/**
 * Background indicator for the settings-page card: the scrim colour of any
 * mode that carries an image (light preferred), or null when the theme has
 * no background at all. Metadata only — never triggers an image fetch.
 */
export function backgroundIndicator(entry: ThemeEntry): ThemeColorSpec | null {
  const light = entry.background?.light;
  const dark = entry.background?.dark;
  if (light?.hasImage && light.scrim) return light.scrim;
  if (dark?.hasImage && dark.scrim) return dark.scrim;
  return null;
}

/**
 * Build the background variable pairs for an injected custom theme. The data
 * URL is assembled exclusively from the daemon-returned, validated MIME type
 * and bytes (R10): nothing from the theme file is ever interpolated into a
 * URL. The scrim is a validated colour spec, rendered as an rgba string the
 * same way palette tokens are.
 */
export function backgroundCssPairs(
  scrim: ThemeColorSpec,
  payload: ThemeBackgroundPayload,
): Array<[string, string]> {
  return [
    [BACKGROUND_PROPERTY_IMAGE, `url("data:${payload.mimeType};base64,${payload.dataB64}")`],
    [BACKGROUND_PROPERTY_SCRIM, colorSpecCssValue(scrim.hex, scrim.opacity)],
  ];
}

/**
 * Inline style for the settings-page preview block of a custom theme: the
 * light palette variables plus the preview surface mappings used by
 * `.palette-card-preview` (so the shared preview layout renders with the
 * custom palette without any per-theme CSS block).
 */
export function customPreviewStyle(entry: ThemeEntry): Record<string, string> {
  const { pairs } = buildThemeCss(entry, "light");
  const style: Record<string, string> = {};
  for (const [name, value] of pairs) {
    if (name.startsWith("--")) style[name] = value;
  }
  style["--preview-bg"] = "var(--bg-window)";
  style["--preview-surface"] = "var(--bg-card)";
  style["--preview-ink"] = "var(--text-primary)";
  style["--preview-line"] = "var(--border-strong)";
  style["--preview-accent"] = "var(--brand)";
  return style;
}

/** Best localised display name for a custom theme entry. */
export function themeDisplayName(entry: ThemeEntry, locale: string): string {
  return entry.name[locale] ?? entry.name.en ?? entry.id;
}
