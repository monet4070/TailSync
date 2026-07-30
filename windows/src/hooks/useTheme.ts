import { useState, useEffect, useCallback, useLayoutEffect } from "react";

export type ThemePreference = "light" | "dark" | "system";
export const COLOR_THEMES = [
  "tailsync",
  "ocean",
  "forest",
  "rose",
  "high-contrast",
] as const;
export type ColorTheme = (typeof COLOR_THEMES)[number];

const STORAGE_KEY = "tailsync-theme";
const COLOR_THEME_STORAGE_KEY = "tailsync-color-theme";

export function isThemePreference(value: string): value is ThemePreference {
  return value === "light" || value === "dark" || value === "system";
}

export function isColorTheme(value: string): value is ColorTheme {
  return COLOR_THEMES.some((theme) => theme === value);
}

function readStoredTheme(): ThemePreference | null {
  try {
    if (typeof localStorage === "undefined") return null;
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored && isThemePreference(stored) ? stored : null;
  } catch {
    return null;
  }
}

function readStoredColorTheme(): ColorTheme {
  try {
    if (typeof localStorage === "undefined") return "tailsync";
    const stored = localStorage.getItem(COLOR_THEME_STORAGE_KEY);
    return stored && isColorTheme(stored) ? stored : "tailsync";
  } catch {
    return "tailsync";
  }
}

function storeValue(key: string, value: string) {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(key, value);
    }
  } catch {
    // Storage can be unavailable in hardened or private webviews.
  }
}

export function useTheme() {
  const [themePreference, setThemeState] = useState<ThemePreference>(
    () => readStoredTheme() || "system",
  );
  const [colorTheme, setColorThemeState] = useState<ColorTheme>(readStoredColorTheme);

  const getEffectiveTheme = useCallback((): "light" | "dark" => {
    if (themePreference === "system") {
      return window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
    }
    return themePreference;
  }, [themePreference]);

  const [effective, setEffective] = useState(getEffectiveTheme);

  useLayoutEffect(() => {
    const app = document.querySelector<HTMLElement>(".app");
    if (!app) return;

    const styles = getComputedStyle(app);
    const background =
      styles.getPropertyValue("--bg-window").trim() || styles.backgroundColor;
    document.documentElement.style.backgroundColor = background;
    document.body.style.backgroundColor = background;
    document.getElementById("root")?.style.setProperty("background-color", background);
  }, [effective, colorTheme]);

  useEffect(() => {
    setEffective(getEffectiveTheme());
  }, [themePreference, getEffectiveTheme]);

  // Listen for system theme changes
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => {
      if (themePreference === "system") {
        setEffective(mq.matches ? "dark" : "light");
      }
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [themePreference]);

  // Listen for localStorage changes from *other* windows (e.g. Settings
  // window changes the theme → History window picks it up).
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY && e.newValue && isThemePreference(e.newValue)) {
        setThemeState(e.newValue);
      }
      if (
        e.key === COLOR_THEME_STORAGE_KEY &&
        e.newValue &&
        isColorTheme(e.newValue)
      ) {
        setColorThemeState(e.newValue);
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  const setTheme = useCallback((t: ThemePreference) => {
    setThemeState(t);
    storeValue(STORAGE_KEY, t);
  }, []);

  const setColorTheme = useCallback((value: ColorTheme) => {
    setColorThemeState(value);
    storeValue(COLOR_THEME_STORAGE_KEY, value);
  }, []);

  return {
    theme: effective,
    setTheme,
    themePreference,
    colorTheme,
    setColorTheme,
  };
}
