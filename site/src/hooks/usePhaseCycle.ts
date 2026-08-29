import { useCallback, useEffect, useRef, useState } from "react";
import { useReducedMotion } from "./useReducedMotion";

/** Ratio above which a set piece is considered "on stage" and allowed to run. */
const RUN_RATIO = 0.2;

type PhaseCycle<T extends Element> = {
  /** Current step index. */
  phase: number;
  /** Jump to a step — used by the clickable timelines. */
  setPhase: (next: number) => void;
  /** Attach to the element whose visibility gates the cycle. */
  ref: React.RefObject<T | null>;
};

/**
 * Drives the stepped "demo" set pieces (handshake, wake recovery, history
 * classifier, product window).
 *
 * These used to be bare `setInterval`s started at mount, which meant every
 * sequence advanced for the whole session no matter where the reader was. Two
 * things went wrong: the page burned a render roughly every 350ms across the
 * four of them, and by the time you scrolled to a section you caught its story
 * mid-cycle instead of at step 01.
 *
 * So the cycle only advances while the element is meaningfully on screen AND
 * the tab is foregrounded, and it rewinds to step 0 once the element is fully
 * out of view — scrolling back gives you the sequence from the top.
 */
export function usePhaseCycle<T extends Element = HTMLDivElement>(
  stepCount: number,
  intervalMs: number,
): PhaseCycle<T> {
  const [phase, setPhase] = useState(0);
  const [pinned, setPinned] = useState(false);
  const ref = useRef<T | null>(null);
  const reducedMotion = useReducedMotion();

  useEffect(() => {
    const element = ref.current;
    if (!element || reducedMotion || stepCount < 2) return;

    let timer = 0;
    let onStage = false;

    const stop = () => {
      window.clearInterval(timer);
      timer = 0;
    };
    const start = () => {
      if (timer || pinned || !onStage || document.visibilityState !== "visible") return;
      timer = window.setInterval(
        () => setPhase((current) => (current + 1) % stepCount),
        intervalMs,
      );
    };

    // Two thresholds from one observer: 0.2 decides whether the sequence runs,
    // and 0 means it is far enough off screen to rewind unseen.
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          onStage = entry.intersectionRatio >= RUN_RATIO;
          if (onStage) start();
          else stop();

          // Rewind on EXIT, not on re-entry. Several sequences transition
          // backwards visibly — the wax seal in SecurityHandshake un-melts over
          // 0.8s, RecoverySequence un-draws its cardiograph over 1.1s — so the
          // reverse has to play while nobody is looking. It also releases the
          // pin, so scrolling away and back resumes the demo.
          if (entry.intersectionRatio === 0) {
            setPhase(0);
            setPinned(false);
          }
        }
      },
      { threshold: [0, RUN_RATIO] },
    );
    observer.observe(element);

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") start();
      else stop();
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      stop();
      observer.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [intervalMs, pinned, reducedMotion, stepCount]);

  // Three of these four sequences have clickable steps that write the same
  // state the timer drives, so an unpinned click was silently overruled within
  // ~1.3s. A deliberate choice now holds until the reader leaves the section.
  const selectPhase = useCallback((next: number) => {
    setPhase(next);
    setPinned(true);
  }, []);

  return { phase, setPhase: selectPhase, ref };
}
