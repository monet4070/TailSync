import { useEffectEvent, useLayoutEffect, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { rowPopoverPosition } from "./historyRowPopoverPosition";

const POPOVER_OPEN_EVENT = "tailsync:history-row-popover-open";

interface HistoryRowPopoverProps {
  id: string;
  anchor: RefObject<HTMLButtonElement | null>;
  label: string;
  labelledBy?: string;
  kind: "actions" | "details";
  onDismiss: () => void;
  children: ReactNode;
}

/** The top layer escapes scroll clipping while inheriting the window's theme.
 * It remains inside this WebView's native bounds and never participates in row layout. */
export function HistoryRowPopover({ id, anchor, label, labelledBy, kind, onDismiss, children }: HistoryRowPopoverProps) {
  const panel = useRef<HTMLDivElement>(null);
  const dismiss = useEffectEvent(onDismiss);

  useLayoutEffect(() => {
    const element = panel.current;
    const trigger = anchor.current;
    if (!element || !trigger) return;
    if (typeof element.showPopover === "function") element.showPopover();
    const position = () => {
      const rect = trigger.getBoundingClientRect();
      const result = rowPopoverPosition(rect, {
        width: element.getBoundingClientRect().width,
        height: element.scrollHeight + 2,
      }, { width: window.innerWidth, height: window.innerHeight });
      element.style.left = `${result.left}px`;
      element.style.top = `${result.top}px`;
      element.style.maxHeight = `${result.maxHeight}px`;
      element.dataset.side = result.side;
      element.style.visibility = "visible";
    };
    position();
    element.focus({ preventScroll: true });

    const outside = (event: Event) => {
      if (event.target instanceof Node && !element.contains(event.target) && !trigger.contains(event.target)) dismiss();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      dismiss();
      trigger.focus({ preventScroll: true });
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !element.contains(event.target)) dismiss();
    };
    const blur = () => dismiss();
    const otherPopover = (event: Event) => {
      if ((event as CustomEvent<string>).detail !== id) dismiss();
    };
    document.addEventListener(POPOVER_OPEN_EVENT, otherPopover);
    document.dispatchEvent(new CustomEvent(POPOVER_OPEN_EVENT, { detail: id }));
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside);
    document.addEventListener("keydown", escape, true);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", position);
    window.addEventListener("blur", blur);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(position);
    observer?.observe(element);
    return () => {
      observer?.disconnect();
      document.removeEventListener(POPOVER_OPEN_EVENT, otherPopover);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside);
      document.removeEventListener("keydown", escape, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", position);
      window.removeEventListener("blur", blur);
      if (typeof element.hidePopover === "function") element.hidePopover();
    };
  }, [anchor, id]);

  const themeRoot = document.querySelector(".app") ?? document.body;
  return createPortal(
    <div ref={panel} id={id} className={`history-row-popover history-row-popover-${kind}`}
      popover={typeof HTMLElement.prototype.showPopover === "function" ? "manual" : undefined}
      tabIndex={-1} role="group" aria-label={labelledBy ? undefined : label}
      aria-labelledby={labelledBy}
      onClick={event => event.stopPropagation()}
      onDoubleClick={event => event.stopPropagation()}
      onContextMenu={event => { event.preventDefault(); event.stopPropagation(); }}
      onPointerDown={event => event.stopPropagation()}
      onKeyDown={event => {
        event.stopPropagation();
        if (kind !== "actions" || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
        const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
          : current < 0 ? (event.key === "ArrowDown" ? 0 : buttons.length - 1)
            : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[index]?.focus({ preventScroll: true });
      }}>
      {children}
    </div>, themeRoot,
  );
}
