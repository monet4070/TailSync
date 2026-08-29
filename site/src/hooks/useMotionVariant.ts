import { useCallback, useEffect, useState } from "react";

export const MOTION_VARIANTS = [
  { id: "base", name: "原版", note: "当前线上的动效语言（含底层修复）" },
  { id: "restrained", name: "收敛", note: "更短、更小、更少 — 编辑感" },
  { id: "draft", name: "制图", note: "24° 擦入 · 线条自绘 · 数字滚动" },
  { id: "weighted", name: "配重", note: "真弹簧 · 质量分级 · 方向语义" },
  { id: "scrolled", name: "随滚", note: "滚动即时间轴 · 可逆回放" },
] as const;

export type MotionVariant = (typeof MOTION_VARIANTS)[number]["id"];

const STORAGE_KEY = "tailsync-motion-variant";
const SCROLL_KEY = "tailsync-motion-scroll";
const DEFAULT_VARIANT: MotionVariant = "base";

function isMotionVariant(value: string | null): value is MotionVariant {
  return MOTION_VARIANTS.some((variant) => variant.id === value);
}

function getInitialVariant(): MotionVariant {
  if (typeof window === "undefined") return DEFAULT_VARIANT;

  const override = new URLSearchParams(window.location.search).get("motion");
  if (isMotionVariant(override)) return override;

  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (isMotionVariant(stored)) return stored;
  } catch {
    // Storage can be unavailable in hardened browser contexts.
  }

  return DEFAULT_VARIANT;
}

/**
 * Stamps `data-motion` on <html> before the first paint.
 *
 * This has to happen outside React. Setting the attribute from an effect means
 * the first frame renders with no variant — every `var(--m-…)` falls back to the
 * `base` literal — and when the attribute lands a frame later, the changed
 * custom properties retrigger the very `transform` and `opacity` transitions
 * they feed. The result is a real transition from base's 32px to the variant's
 * offset, so the page opens by animating between two motion languages.
 */
export function applyMotionVariantEarly(): void {
  if (!import.meta.env.DEV) return;
  document.documentElement.dataset.motion = getInitialVariant();
}

/**
 * Selects which motion language the page speaks.
 *
 * The variants live in `src/styles/motion.css` behind `[data-motion="…"]`, so
 * the attribute alone is enough to restyle the page. Switching still reloads,
 * though, and that is the point: `App.tsx` reveals each element once and then
 * `unobserve`s it, so a live swap would leave every section already revealed and
 * you would be comparing resting states instead of entrances. The reload replays
 * them, and scroll position is carried across so you land where you were.
 */
export function useMotionVariant(): {
  variant: MotionVariant;
  selectVariant: (next: MotionVariant) => void;
} {
  const [variant, setVariant] = useState<MotionVariant>(getInitialVariant);

  useEffect(() => {
    let restored = 0;
    try {
      restored = Number(window.sessionStorage.getItem(SCROLL_KEY) ?? 0);
      window.sessionStorage.removeItem(SCROLL_KEY);
    } catch {
      return;
    }
    if (!restored) return;

    // Jump, don't glide — base.css sets `scroll-behavior: smooth`, which would
    // otherwise animate all the way down and trip every reveal on the way.
    const root = document.documentElement;
    const behavior = root.style.scrollBehavior;
    root.style.scrollBehavior = "auto";
    window.scrollTo(0, restored);
    root.style.scrollBehavior = behavior;
  }, []);

  const selectVariant = useCallback((next: MotionVariant) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
      window.sessionStorage.setItem(SCROLL_KEY, String(Math.round(window.scrollY)));
    } catch {
      // Without storage the reload would land on the previous variant, so apply
      // it in place and accept that already-revealed sections stay revealed.
      setVariant(next);
      return;
    }
    window.location.reload();
  }, []);

  return { variant, selectVariant };
}
