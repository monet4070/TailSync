import { useEffect, useState } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

/**
 * Live `prefers-reduced-motion` state.
 *
 * The site previously read this media query once per component at mount, so
 * flipping the OS setting mid-session had no effect until a reload. This
 * subscribes to `change` the way `windows/src/hooks/useTheme.ts` already does,
 * which lets the motion layer react immediately.
 */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => typeof window !== "undefined" && window.matchMedia(QUERY).matches,
  );

  useEffect(() => {
    const query = window.matchMedia(QUERY);
    const handleChange = () => setReduced(query.matches);
    handleChange();
    query.addEventListener("change", handleChange);
    return () => query.removeEventListener("change", handleChange);
  }, []);

  return reduced;
}
