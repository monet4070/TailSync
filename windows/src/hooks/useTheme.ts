import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getLocalThemeSettingsV2, getThemeAssetSlot, resolveThemeV2, setLocalThemeSettingsV2 } from "../tailsyncClient";
import { themeV2CssPairs, themeV2CssProperties } from "../utils/themeV2Css";

export type ThemePreference = "light" | "dark" | "system";
export type ColorTheme = string;

function isThemePreference(value: string): value is ThemePreference { return value === "light" || value === "dark" || value === "system"; }

export function useTheme() {
  const [themePreference, setThemeState] = useState<ThemePreference>("system");
  const [colorTheme, setColorThemeState] = useState<ColorTheme>("builtin:canvas@1");
  const [highContrastPreference, setHighContrastPreference] = useState(false);
  const [systemHighContrast, setSystemHighContrast] = useState(() => window.matchMedia("(forced-colors: active)").matches);
  const [systemReduceTransparency, setSystemReduceTransparency] = useState(() => window.matchMedia("(prefers-reduced-transparency: reduce)").matches);
  const [themeAssetSlots, setThemeAssetSlots] = useState<Record<string, boolean>>({});
  useEffect(() => {
    let active = true; let unlisten: (() => void) | undefined;
    void getLocalThemeSettingsV2().then((settings) => { if (active) { setColorThemeState(settings.activeThemeId); setThemeState(settings.appearance); setHighContrastPreference(Boolean(settings.highContrast)); } }).catch(() => { if (active) setColorThemeState("builtin:canvas@1"); });
    void listen<{ activeThemeId: string; appearance: ThemePreference; highContrast: boolean }>("theme_changed", ({ payload }) => { if (!active) return; setColorThemeState(payload.activeThemeId || "builtin:canvas@1"); if (isThemePreference(payload.appearance)) setThemeState(payload.appearance); setHighContrastPreference(Boolean(payload.highContrast)); }).then((stop) => { if (active) unlisten = stop; else stop(); });
    return () => { active = false; unlisten?.(); };
  }, []);
  const getEffectiveTheme = useCallback((): "light" | "dark" => themePreference === "system" ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : themePreference, [themePreference]);
  const [effective, setEffective] = useState(getEffectiveTheme);
  const highContrast = systemHighContrast || highContrastPreference;
  useLayoutEffect(() => {
    let cancelled = false; const app = document.querySelector<HTMLElement>(".app"); if (!app) return;
    const assetUrls: string[] = [];
    const apply = async () => {
      const resolved = highContrast
        ? await resolveThemeV2(colorTheme, effective, true)
        : await resolveThemeV2(colorTheme, effective);
      if (cancelled) return;
      themeV2CssProperties.forEach(name => app.style.removeProperty(name));
      for (const [name, value] of themeV2CssPairs(resolved.tokens as Record<string, any>, { reduceTransparency: systemReduceTransparency, mode: effective })) app.style.setProperty(name, value);
      app.toggleAttribute("data-theme-high-contrast", highContrast);
      const slotProperties: Record<string, string> = { logo: "--theme-logo-image", emptyState: "--theme-empty-state-image", previewPlaceholder: "--theme-preview-placeholder-image" };
      const available: Record<string, boolean> = {};
      await Promise.all(Object.entries(slotProperties).map(async ([slot, property]) => {
        const asset = resolved.assetSlots?.[slot];
        if (!asset) return;
        try {
          const bytes = await getThemeAssetSlot(colorTheme, resolved.digest, slot);
          if (cancelled) return;
          const url = URL.createObjectURL(new Blob([bytes], { type: asset.mimeType }));
          assetUrls.push(url);
          app.style.setProperty(property, `url("${url}")`);
          available[slot] = true;
        } catch {
          // A missing or stale image only falls back to the built-in glyph.
        }
      }));
      if (!cancelled) setThemeAssetSlots(available);
    };
    void apply().catch(() => { void setLocalThemeSettingsV2({ activeThemeId: "builtin:canvas@1", appearance: themePreference, highContrast: highContrastPreference }); setColorThemeState("builtin:canvas@1"); });
    return () => { cancelled = true; setThemeAssetSlots({}); app.removeAttribute("data-theme-high-contrast"); assetUrls.forEach(url => URL.revokeObjectURL(url)); };
  }, [colorTheme, effective, highContrast, highContrastPreference, systemReduceTransparency, themePreference]);
  useEffect(() => {
    const mq = window.matchMedia("(forced-colors: active)");
    const handler = () => setSystemHighContrast(mq.matches);
    handler(); mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const app = document.querySelector<HTMLElement>(".app"); if (!app) return;
    const handler = () => app.toggleAttribute("data-reduced-motion", mq.matches);
    handler(); mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-transparency: reduce)");
    const app = document.querySelector<HTMLElement>(".app"); if (!app) return;
    const handler = () => { setSystemReduceTransparency(mq.matches); app.toggleAttribute("data-reduced-transparency", mq.matches); };
    handler(); mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  useLayoutEffect(() => { const app = document.querySelector<HTMLElement>(".app"); if (!app) return; const background = getComputedStyle(app).getPropertyValue("--bg-window").trim() || getComputedStyle(app).backgroundColor; document.documentElement.style.backgroundColor = background; document.body.style.backgroundColor = background; document.getElementById("root")?.style.setProperty("background-color", background); }, [effective, colorTheme]);
  useEffect(() => { setEffective(getEffectiveTheme()); }, [themePreference, getEffectiveTheme]);
  useEffect(() => { const mq = window.matchMedia("(prefers-color-scheme: dark)"); const handler = () => { if (themePreference === "system") setEffective(mq.matches ? "dark" : "light"); }; mq.addEventListener("change", handler); return () => mq.removeEventListener("change", handler); }, [themePreference]);
  const persist = useCallback((activeThemeId: string, appearance: ThemePreference) => { void setLocalThemeSettingsV2({ activeThemeId, appearance, highContrast: highContrastPreference }).catch(() => { setThemeState("system"); setColorThemeState("builtin:canvas@1"); }); }, [highContrastPreference]);
  const setTheme = useCallback((appearance: ThemePreference) => { setThemeState(appearance); persist(colorTheme, appearance); }, [colorTheme, persist]);
  const setColorTheme = useCallback((activeThemeId: ColorTheme) => { setColorThemeState(activeThemeId); persist(activeThemeId, themePreference); }, [persist, themePreference]);
  return { theme: effective, setTheme, themePreference, colorTheme, setColorTheme, resolvedColorTheme: colorTheme, highContrast, themeAssetSlots };
}
