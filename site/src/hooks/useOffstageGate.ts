import { useEffect } from "react";

/**
 * Marks whole sections as off stage so their ambient loops can stand down.
 *
 * The page carries 31 `infinite` animations — orbits, radar pings, marquees,
 * scan sheens, six `live-pulse` dots — and none of them used to stop. Below the
 * fold they were still compositing, and several (`box-shadow` pings,
 * `background-position` sheens) repaint rather than composite, so the cost was
 * real on modest hardware and on battery.
 *
 * Gating at section level means one CSS rule can reach every animated
 * descendant *and* pseudo-element, which per-element bookkeeping cannot:
 *
 *   [data-offstage="true"] *, …::before, …::after { animation-play-state: paused }
 *
 * Pausing rather than cancelling keeps each loop's phase, so a section picks up
 * mid-orbit exactly where it left off instead of snapping back to 0%.
 */
export function useOffstageGate(): void {
  useEffect(() => {
    const blocks = [
      ...document.querySelectorAll<HTMLElement>("main > section, main > div, footer"),
    ];
    if (blocks.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const element = entry.target as HTMLElement;
          if (entry.isIntersecting) element.removeAttribute("data-offstage");
          else element.dataset.offstage = "true";
        }
      },
      // A little slack so a loop is already running by the time its section
      // edges into view, rather than visibly kicking off mid-scroll.
      { rootMargin: "120px 0px", threshold: 0 },
    );
    blocks.forEach((block) => observer.observe(block));
    return () => observer.disconnect();
  }, []);
}
