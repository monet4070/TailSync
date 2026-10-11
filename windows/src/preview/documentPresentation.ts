import type * as Contract from "../types/localContracts.generated";

function clampColorChannel(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(255, Math.trunc(value)));
}

export function cssColor(color: Contract.DocumentColor): string {
  return `rgb(${clampColorChannel(color.red)}, ${clampColorChannel(color.green)}, ${clampColorChannel(color.blue)})`;
}

function linearChannel(value: number): number {
  return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance(color: Contract.DocumentColor): number {
  return 0.2126 * linearChannel(clampColorChannel(color.red) / 255)
    + 0.7152 * linearChannel(clampColorChannel(color.green) / 255)
    + 0.0722 * linearChannel(clampColorChannel(color.blue) / 255);
}

export function contrastRatio(foreground: Contract.DocumentColor, background: Contract.DocumentColor): number {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  return (Math.max(foregroundLuminance, backgroundLuminance) + 0.05)
    / (Math.min(foregroundLuminance, backgroundLuminance) + 0.05);
}

export function readableTextColor(highlight: Contract.DocumentColor): string {
  return relativeLuminance(highlight) > 0.1791 ? "rgb(0, 0, 0)" : "rgb(255, 255, 255)";
}

const linkByteEncoder = new TextEncoder();

export function safeDocumentLink(raw: string | null | undefined): string | null {
  if (typeof raw !== "string" || raw.length === 0) return null;
  if (linkByteEncoder.encode(raw).length > 2048 || raw.includes("\\")) return null;
  for (const character of raw) {
    const scalar = character.codePointAt(0) ?? 0;
    if (character.trim() === "" || scalar <= 31 || (scalar >= 127 && scalar <= 159)) return null;
  }
  const authority = raw.match(/^https?:\/\/([^/?#]*)/i)?.[1];
  if (!authority || authority.includes("@")) return null;
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  if (parsed.username !== "" || parsed.password !== "" || parsed.hostname === "") return null;
  return parsed.href;
}

export function headingLevel(value: number | null | undefined): 1 | 2 | 3 | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 1) return null;
  if (value >= 3) return 3;
  return value === 1 ? 1 : 2;
}
