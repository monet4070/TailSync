import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  getThemeBackground,
  listThemes,
  type ThemeBackgroundPayload,
  type ThemeEntry,
  type ThemeErrorItem,
} from "../tailsyncClient";
import {
  allCustomThemeProperties,
  backgroundCssPairs,
  buildThemeCss,
} from "../utils/themeCss";

export type ThemePreference = "light" | "dark" | "system";
export const COLOR_THEMES = [
  "tailsync",
  "ocean",
  "forest",
  "rose",
  "high-contrast",
] as const;
export type BuiltinColorTheme = (typeof COLOR_THEMES)[number];

/** Namespace prefix for custom themes stored in settings/localStorage. */
export const CUSTOM_THEME_PREFIX = "custom:";

const CUSTOM_THEME_PATTERN = /^custom:[a-z0-9][a-z0-9-]{0,31}$/;
export type CustomColorTheme = `${typeof CUSTOM_THEME_PREFIX}${string}`;
export type ColorTheme = BuiltinColorTheme | CustomColorTheme;

const STORAGE_KEY = "tailsync-theme";
const COLOR_THEME_STORAGE_KEY = "tailsync-color-theme";

export function isThemePreference(value: string): value is ThemePreference {
  return value === "light" || value === "dark" || value === "system";
}

export function isCustomColorTheme(value: string): value is CustomColorTheme {
  return CUSTOM_THEME_PATTERN.test(value);
}

export function isColorTheme(value: string): value is ColorTheme {
  return COLOR_THEMES.some((theme) => theme === value) || isCustomColorTheme(value);
}

/** The bare id of a `custom:` value, or null when it is not custom. */
export function customThemeId(value: string): string | null {
  return value.startsWith(CUSTOM_THEME_PREFIX)
    ? value.slice(CUSTOM_THEME_PREFIX.length)
    : null;
}

/**
 * Resolve the theme id to apply: built-ins pass through, custom ids that are
 * present in the loaded catalogue pass through, anything else (unknown custom
 * ids, invalid values) falls back to the default theme. The stored value is
 * never rewritten — fallback happens only at apply time.
 */
export function resolveColorTheme(
  value: string,
  availableCustomIds: ReadonlySet<string>,
): ColorTheme {
  if (isColorTheme(value)) {
    const custom = customThemeId(value);
    if (custom === null || availableCustomIds.has(custom)) return value;
  }
  return "tailsync";
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
    if (!stored) return "tailsync";
    if (COLOR_THEMES.some((theme) => theme === stored)) return stored as BuiltinColorTheme;
    // Unknown custom ids keep their stored value here; the fallback to the
    // default theme happens when the value is applied (resolveColorTheme).
    if (isCustomColorTheme(stored)) return stored;
    return "tailsync";
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
  const [customThemes, setCustomThemes] = useState<ThemeEntry[]>([]);
  const [themeLoadErrors, setThemeLoadErrors] = useState<ThemeErrorItem[]>([]);

  // Reload the custom-theme catalogue after import/delete operations.
  const refreshCustomThemes = useCallback(() => {
    listThemes()
      .then((listing) => {
        setCustomThemes(listing.custom);
        setThemeLoadErrors(listing.errors);
      })
      .catch(() => {
        setCustomThemes([]);
        setThemeLoadErrors([]);
      });
  }, []);

  // Load the custom-theme catalogue from the daemon once per window. A
  // failing daemon leaves the catalogue empty (fallback behaviour).
  useEffect(() => {
    refreshCustomThemes();
  }, [refreshCustomThemes]);

  const customThemeIds = useMemo(
    () => new Set(customThemes.map((entry) => entry.id)),
    [customThemes],
  );

  // The id actually applied: unknown custom ids fall back to the default.
  const resolvedColorTheme = useMemo(
    () => resolveColorTheme(colorTheme, customThemeIds),
    [colorTheme, customThemeIds],
  );

  const getEffectiveTheme = useCallback((): "light" | "dark" => {
    if (themePreference === "system") {
      return window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
    }
    return themePreference;
  }, [themePreference]);

  const [effective, setEffective] = useState(getEffectiveTheme);

  // R006: inject the custom theme's CSS variables when a custom theme is
  // active. Runs before the background effect below so computed styles see
  // the injected palette. Switching back to a built-in theme (or to an
  // unknown custom id) removes exactly the injected properties.
  useLayoutEffect(() => {
    const app = document.querySelector<HTMLElement>(".app");
    if (!app) return;
    const custom = customThemeId(colorTheme);
    const entry = custom
      ? customThemes.find((candidate) => candidate.id === custom)
      : undefined;
    for (const name of allCustomThemeProperties()) {
      app.style.removeProperty(name);
    }
    if (!entry) return;
    const { pairs, warnings } = buildThemeCss(entry, effective);
    for (const warning of warnings) {
      console.warn(`TailSync: ${warning}`);
    }
    for (const [name, value] of pairs) {
      app.style.setProperty(name, value);
    }
  }, [colorTheme, customThemes, effective]);

  // R005: fetch and inject the custom theme's background image + scrim for
  // the current light/dark mode. The payload comes from the daemon's
  // validated `get_theme_background`; the data URL is assembled here from
  // the validated MIME type and bytes only. The palette effect above clears
  // these variables when the theme or mode changes; an in-flight fetch for
  // a stale selection is dropped via the cancellation flag. Fetches are
  // cached for the session and discarded when the theme selection changes.
  const backgroundCache = useRef(new Map<string, ThemeBackgroundPayload>());
  useEffect(() => {
    backgroundCache.current.clear();
  }, [colorTheme]);

  useEffect(() => {
    let cancelled = false;
    const app = document.querySelector<HTMLElement>(".app");
    if (!app) return;
    const custom = customThemeId(colorTheme);
    const entry = custom
      ? customThemes.find((candidate) => candidate.id === custom)
      : undefined;
    const meta = entry?.background
      ? effective === "light"
        ? entry.background.light
        : entry.background.dark
      : undefined;
    if (!entry || !meta?.hasImage || !meta.scrim) return;
    const key = `${entry.id}:${effective}`;
    const cached = backgroundCache.current.get(key);
    const apply = (payload: ThemeBackgroundPayload) => {
      if (cancelled) return;
      for (const [name, value] of backgroundCssPairs(meta.scrim!, payload)) {
        app.style.setProperty(name, value);
      }
    };
    if (cached) {
      apply(cached);
      return;
    }
    getThemeBackground(entry.id, effective)
      .then((payload) => {
        if (cancelled || !payload) return;
        backgroundCache.current.set(key, payload);
        apply(payload);
      })
      .catch(() => {
        // Daemon unavailable: keep the default flat background.
      });
    return () => {
      cancelled = true;
    };
  }, [colorTheme, customThemes, effective]);

  useLayoutEffect(() => {
    const app = document.querySelector<HTMLElement>(".app");
    if (!app) return;

    const styles = getComputedStyle(app);
    const background =
      styles.getPropertyValue("--bg-window").trim() || styles.backgroundColor;
    document.documentElement.style.backgroundColor = background;
    document.body.style.backgroundColor = background;
    document.getElementById("root")?.style.setProperty("background-color", background);
  }, [effective, colorTheme, resolvedColorTheme]);

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
    resolvedColorTheme,
    customThemes,
    themeLoadErrors,
    refreshCustomThemes,
  };
}
