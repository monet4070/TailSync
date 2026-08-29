import type { CSSProperties } from "react";
import type { ThemeV2Descriptor } from "../../tailsyncClient";

export const palettePreviewClass = (themeId: string) => {
  switch (themeId) {
    case "builtin:flux@1": return "ocean";
    case "builtin:ledger@1": return "forest";
    case "builtin:aura@1": return "rose";
    case "builtin:mono@1": return "high-contrast";
    case "builtin:canvas@1": return "tailsync";
    default: return "custom";
  }
};

export const palettePreviewTitle = (themeId: string) =>
  themeId === "builtin:flux@1" || themeId === "builtin:mono@1"
    ? "TAILSYNC"
    : "TailSync";

export const themeToken = (
  tokens: Record<string, unknown> | undefined,
  path: string[],
): string | undefined => {
  let current: unknown = tokens;
  for (const key of path) {
    if (!current || typeof current !== "object" || Array.isArray(current)) return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return typeof current === "string" ? current : undefined;
};

export const themeTokenFamilies = (
  tokens: Record<string, unknown> | undefined,
  path: string[],
): string | undefined => {
  let current: unknown = tokens;
  for (const key of path) {
    if (!current || typeof current !== "object" || Array.isArray(current)) return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return Array.isArray(current) && current.every((value) => typeof value === "string")
    ? current.join(", ")
    : undefined;
};

export const palettePreviewStyle = (entry: ThemeV2Descriptor): CSSProperties | undefined => {
  const tokens = entry.resolvedLight?.tokens;
  if (!tokens) return undefined;
  const values: Record<string, string | undefined> = {
    "--preview-bg": themeToken(tokens, ["colors", "background", "canvas"]),
    "--preview-surface": themeToken(tokens, ["colors", "background", "surface"]),
    "--preview-ink": themeToken(tokens, ["colors", "text", "primary"]),
    "--preview-secondary": themeToken(tokens, ["colors", "text", "secondary"]),
    "--preview-line": themeToken(tokens, ["colors", "border", "default"]),
    "--preview-border-strong": themeToken(tokens, ["colors", "border", "strong"]),
    "--preview-accent": themeToken(tokens, ["colors", "accent", "default"]),
    "--preview-accent-soft": themeToken(tokens, ["colors", "accent", "soft"]),
    "--preview-input": themeToken(tokens, ["colors", "background", "input"]),
    "--preview-title-font": themeTokenFamilies(tokens, ["typography", "display", "families"]),
  };
  return Object.fromEntries(
    Object.entries(values).filter(([, value]) => value !== undefined),
  ) as CSSProperties;
};
