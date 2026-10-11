const EDGE = 8;
const GAP = 6;

export function rowPopoverPosition(
  anchor: Pick<DOMRect, "left" | "right" | "top" | "bottom">,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
) {
  const below = Math.max(0, viewport.height - EDGE - anchor.bottom - GAP);
  const above = Math.max(0, anchor.top - GAP - EDGE);
  const side = below >= size.height || below >= above ? "bottom" : "top";
  const maxHeight = side === "bottom" ? below : above;
  const height = Math.min(size.height, maxHeight);
  const width = Math.min(size.width, Math.max(0, viewport.width - 2 * EDGE));
  return {
    left: Math.max(EDGE, Math.min(anchor.right - width, viewport.width - EDGE - width)),
    top: side === "bottom" ? anchor.bottom + GAP : anchor.top - GAP - height,
    maxHeight,
    side,
  };
}
