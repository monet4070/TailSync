/**
 * Keep expensive browser renderers bounded even though the transport preview
 * limit is intentionally larger for copy/restore and other non-DOM callers.
 */
export const TEXT_PREVIEW_RENDER_MAX_CHARS = 256 * 1024;
export const TEXT_PREVIEW_HIGHLIGHT_MAX_CHARS = 64 * 1024;
export const TEXT_PREVIEW_MAX_LINE_NUMBER_ROWS = 5_000;

export interface LimitedTextSource {
  source: string;
  truncated: boolean;
}

export function limitTextSource(source: string): LimitedTextSource {
  if (source.length <= TEXT_PREVIEW_RENDER_MAX_CHARS) {
    return { source, truncated: false };
  }

  const boundary = source.lastIndexOf("\n", TEXT_PREVIEW_RENDER_MAX_CHARS);
  const end = boundary >= TEXT_PREVIEW_RENDER_MAX_CHARS * 0.75
    ? boundary
    : TEXT_PREVIEW_RENDER_MAX_CHARS;
  return { source: source.slice(0, end), truncated: true };
}

export function countTextLines(source: string): number {
  let count = 1;
  let offset = source.indexOf("\n");
  while (offset >= 0) {
    count += 1;
    offset = source.indexOf("\n", offset + 1);
  }
  return count;
}
