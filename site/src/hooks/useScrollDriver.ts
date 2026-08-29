import { useEffect } from "react";

/** Distance scrolled before the header densifies. */
const HEADER_SHIFT = 24;

/**
 * Publishes scroll position to CSS instead of to React state.
 *
 * `App` used to hold `scrollProgress` in `useState` and update it from a scroll
 * listener, which re-rendered the entire landing tree — every section, every
 * set piece — on every scroll frame, all to drive one 2.5px bar. Here the same
 * numbers land on `documentElement` as custom properties, so the bar and any
 * scroll-linked motion are pure CSS and the React tree stays still:
 *
 *   --scroll-progress  0 → 1 across the whole document
 *   --hero-progress    0 → 1 across the first viewport (hero exit)
 *
 * It also toggles `is-scrolled` on the header, which `base.css:141` has always
 * styled but nothing ever applied.
 */
export function useScrollDriver(headerRef: React.RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const root = document.documentElement;
    let frame = 0;

    const publish = () => {
      frame = 0;
      const maxScroll = root.scrollHeight - window.innerHeight;
      const offset = window.scrollY;
      const progress = maxScroll > 0 ? Math.min(1, Math.max(0, offset / maxScroll)) : 0;
      const heroProgress = window.innerHeight > 0
        ? Math.min(1, Math.max(0, offset / window.innerHeight))
        : 0;

      root.style.setProperty("--scroll-progress", progress.toFixed(5));
      root.style.setProperty("--hero-progress", heroProgress.toFixed(5));
      headerRef.current?.classList.toggle("is-scrolled", offset > HEADER_SHIFT);
    };

    const schedule = () => {
      if (!frame) frame = window.requestAnimationFrame(publish);
    };

    publish();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [headerRef]);
}
