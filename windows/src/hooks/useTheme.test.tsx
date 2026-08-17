import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemeEntry, ThemePalette } from "../tailsyncClient";
import {
  customThemeId,
  isColorTheme,
  isCustomColorTheme,
  isThemePreference,
  resolveColorTheme,
  useTheme,
} from "./useTheme";

const listThemesMock = vi.hoisted(() => vi.fn());
const getThemeBackgroundMock = vi.hoisted(() => vi.fn());

vi.mock("../tailsyncClient", () => ({
  listThemes: listThemesMock,
  getThemeBackground: getThemeBackgroundMock,
}));

class MockMediaQueryList extends EventTarget {
  matches = false;
  readonly media = "(prefers-color-scheme: dark)";
  onchange: ((event: MediaQueryListEvent) => void) | null = null;

  addListener(listener: (event: MediaQueryListEvent) => void) {
    this.addEventListener("change", listener as EventListener);
  }

  removeListener(listener: (event: MediaQueryListEvent) => void) {
    this.removeEventListener("change", listener as EventListener);
  }

  setMatches(matches: boolean) {
    this.matches = matches;
    this.dispatchEvent(new Event("change"));
  }
}

function makePalette(hex: string, opacity: number): ThemePalette {
  const plain = { hex };
  const soft = { hex, opacity };
  return {
    brand: plain,
    brandHover: plain,
    brandSoft: soft,
    brandText: plain,
    bgWindow: plain,
    bgCard: plain,
    bgInput: soft,
    bgHover: soft,
    bgActive: soft,
    bgRaised: plain,
    bgToast: plain,
    textPrimary: plain,
    textSecondary: plain,
    textTertiary: plain,
    textToast: plain,
    border: plain,
    borderStrong: plain,
    divider: plain,
    green: plain,
    greenSoft: soft,
    orange: plain,
    orangeSoft: soft,
    purple: plain,
    purpleSoft: soft,
  };
}

const studioTheme: ThemeEntry = {
  id: "studio",
  name: { en: "Studio" },
  file: "studio.json",
  palette: {
    light: makePalette("#d5684b", 0.11),
    dark: makePalette("#ec8668", 0.14),
  },
  metrics: { cardRadius: 10, controlRadius: 9, rowPadding: 13, shadowRadius: 8 },
  typography: {
    sectionTitleSize: 25,
    uppercasesSectionTitles: false,
    searchSize: 18,
    searchUsesDisplayFont: true,
    historyContentSize: 15,
  },
  fonts: { display: "Songti SC", reading: "Avenir Next" },
  structural: { borderRadius: 10, shadow: false },
  background: {
    light: {
      hasImage: true,
      scrim: { hex: "#0f1526", opacity: 0.82 },
      mimeType: "image/png",
    },
    dark: { hasImage: false },
  },
};

describe("useTheme", () => {
  let mediaQuery: MockMediaQueryList;

  beforeEach(() => {
    localStorage.clear();
    mediaQuery = new MockMediaQueryList();
    vi.stubGlobal("matchMedia", vi.fn(() => mediaQuery));
    listThemesMock.mockReset();
    listThemesMock.mockResolvedValue({ builtin: [], custom: [], errors: [] });
    getThemeBackgroundMock.mockReset();
    getThemeBackgroundMock.mockResolvedValue(null);
    document.body.innerHTML = '<div id="root"></div><div class="app"></div>';
  });

  it("validates persisted theme identifiers", () => {
    expect(isThemePreference("system")).toBe(true);
    expect(isThemePreference("sepia")).toBe(false);
    expect(isColorTheme("high-contrast")).toBe(true);
    expect(isColorTheme("unknown")).toBe(false);
  });

  it("validates custom theme identifiers", () => {
    expect(isCustomColorTheme("custom:studio")).toBe(true);
    expect(isCustomColorTheme("custom:0abc")).toBe(true);
    expect(isCustomColorTheme("custom:my-theme-2")).toBe(true);
    expect(isColorTheme("custom:studio")).toBe(true);
    expect(isCustomColorTheme("custom:")).toBe(false);
    expect(isCustomColorTheme("custom:Studio")).toBe(false);
    expect(isCustomColorTheme("custom:studio!")).toBe(false);
    expect(isCustomColorTheme("custom:studio/evil")).toBe(false);
    expect(isCustomColorTheme("xcustom:studio")).toBe(false);
    expect(isCustomColorTheme(`custom:${"a".repeat(33)}`)).toBe(false);
    expect(customThemeId("custom:studio")).toBe("studio");
    expect(customThemeId("ocean")).toBeNull();
  });

  it("resolves unknown custom ids to the default theme at apply time", () => {
    expect(resolveColorTheme("ocean", new Set())).toBe("ocean");
    expect(resolveColorTheme("custom:studio", new Set(["studio"]))).toBe("custom:studio");
    expect(resolveColorTheme("custom:studio", new Set())).toBe("tailsync");
    expect(resolveColorTheme("garbage", new Set())).toBe("tailsync");
  });

  it("ignores invalid persisted values", () => {
    localStorage.setItem("tailsync-theme", "sepia");
    localStorage.setItem("tailsync-color-theme", "unknown");

    const { result } = renderHook(() => useTheme());

    expect(result.current.themePreference).toBe("system");
    expect(result.current.theme).toBe("light");
    expect(result.current.colorTheme).toBe("tailsync");
  });

  it("persists explicit appearance changes", () => {
    const { result } = renderHook(() => useTheme());

    act(() => {
      result.current.setTheme("dark");
      result.current.setColorTheme("rose");
    });

    expect(result.current.theme).toBe("dark");
    expect(result.current.colorTheme).toBe("rose");
    expect(localStorage.getItem("tailsync-theme")).toBe("dark");
    expect(localStorage.getItem("tailsync-color-theme")).toBe("rose");
  });

  it("tracks system appearance changes in system mode", () => {
    const { result } = renderHook(() => useTheme());

    act(() => mediaQuery.setMatches(true));

    expect(result.current.theme).toBe("dark");
  });

  it("accepts valid appearance changes from another window", () => {
    const { result } = renderHook(() => useTheme());

    act(() => {
      window.dispatchEvent(new StorageEvent("storage", {
        key: "tailsync-theme",
        newValue: "dark",
      }));
      window.dispatchEvent(new StorageEvent("storage", {
        key: "tailsync-color-theme",
        newValue: "forest",
      }));
    });

    expect(result.current.themePreference).toBe("dark");
    expect(result.current.theme).toBe("dark");
    expect(result.current.colorTheme).toBe("forest");
  });

  it("keeps an unknown stored custom id and falls back only when applied", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    // The stored value is preserved (no premature rewrite)…
    expect(result.current.colorTheme).toBe("custom:studio");
    // …while the applied id falls back to the default theme.
    expect(result.current.resolvedColorTheme).toBe("tailsync");
  });

  it("falls back when the daemon has no custom-theme data", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockRejectedValue(new Error("daemon unavailable"));

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    expect(result.current.customThemes).toEqual([]);
    expect(result.current.resolvedColorTheme).toBe("tailsync");
    expect(result.current.colorTheme).toBe("custom:studio");
  });

  it("loads the custom catalogue and resolves known custom ids", async () => {
    listThemesMock.mockResolvedValue({
      builtin: [],
      custom: [studioTheme],
      errors: [{ file: "broken.json", reason: "Invalid theme JSON" }],
    });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    expect(result.current.customThemes).toEqual([studioTheme]);
    expect(result.current.themeLoadErrors).toEqual([
      { file: "broken.json", reason: "Invalid theme JSON" },
    ]);
    expect(result.current.customThemes[0].id).toBe("studio");
  });

  it("refreshes the document backdrop after an async custom catalogue load", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });

    renderHook(() => useTheme());
    await act(async () => {});

    expect(document.documentElement.style.backgroundColor).toBe("rgb(213, 104, 75)");
    expect(document.body.style.backgroundColor).toBe("rgb(213, 104, 75)");
    expect(document.getElementById("root")?.style.backgroundColor).toBe("rgb(213, 104, 75)");
  });

  it("reloads the catalogue on demand after import or delete", async () => {
    const { result } = renderHook(() => useTheme());
    await act(async () => {});
    expect(result.current.customThemes).toEqual([]);

    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });
    act(() => {
      result.current.refreshCustomThemes();
    });
    await act(async () => {});

    expect(listThemesMock).toHaveBeenCalledTimes(2);
    expect(result.current.customThemes).toEqual([studioTheme]);
  });

  it("syncs custom theme values from another window via storage events", () => {
    const { result } = renderHook(() => useTheme());

    act(() => {
      window.dispatchEvent(new StorageEvent("storage", {
        key: "tailsync-color-theme",
        newValue: "custom:studio",
      }));
    });

    expect(result.current.colorTheme).toBe("custom:studio");
  });

  it("injects the full custom-theme variable set onto the app element", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app).not.toBeNull();
    // 24 palette tokens + 2 fonts + 2 structural overrides + 11
    // metrics/typography (R007) = 39 entries.
    expect(app!.style.length).toBe(39);
    expect(app!.style.getPropertyValue("--brand")).toBe("#d5684b");
    expect(app!.style.getPropertyValue("--brand-soft")).toBe("rgba(213, 104, 75, 0.11)");
    expect(app!.style.getPropertyValue("--bg-input")).toBe("rgba(213, 104, 75, 0.11)");
    expect(app!.style.getPropertyValue("--text-primary")).toBe("#d5684b");
    expect(app!.style.getPropertyValue("--art-display")).toBe("Songti SC");
    expect(app!.style.getPropertyValue("--art-reading")).toBe("Avenir Next");
    expect(app!.style.getPropertyValue("border-radius")).toBe("10px");
    expect(app!.style.getPropertyValue("box-shadow")).toBe("none");
    // R007: metrics and typography map deterministically onto the
    // design-system variables (studio: card 10 / control 9 / row 13 /
    // shadow 8, section 25 / content 15 / search 18, display-font search,
    // non-uppercased section titles).
    expect(app!.style.getPropertyValue("--radius-sm")).toBe("9px");
    expect(app!.style.getPropertyValue("--radius-md")).toBe("10px");
    expect(app!.style.getPropertyValue("--window-radius")).toBe("10px");
    expect(app!.style.getPropertyValue("--history-row-padding-y")).toBe("13px");
    expect(app!.style.getPropertyValue("--setting-row-padding-y")).toBe("13px");
    expect(app!.style.getPropertyValue("--shadow-md")).toBe("0 4px 8px rgba(0, 0, 0, 0.08)");
    expect(app!.style.getPropertyValue("--font-size-section")).toBe("25px");
    expect(app!.style.getPropertyValue("--font-size-content")).toBe("15px");
    expect(app!.style.getPropertyValue("--search-font-size")).toBe("18px");
    expect(app!.style.getPropertyValue("--search-font-family")).toBe("var(--font-display)");
    expect(app!.style.getPropertyValue("--section-title-transform")).toBe("none");
    expect(result.current.resolvedColorTheme).toBe("custom:studio");
  });

  it("re-injects with the dark palette when the effective mode changes", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("--brand")).toBe("#d5684b");

    act(() => mediaQuery.setMatches(true));

    expect(result.current.theme).toBe("dark");
    expect(app!.style.getPropertyValue("--brand")).toBe("#ec8668");
    expect(app!.style.getPropertyValue("--brand-soft")).toBe("rgba(236, 134, 104, 0.14)");
  });

  it("applies uppercasesSectionTitles and searchUsesDisplayFont booleans", async () => {
    // R007: a theme that uppercases its section titles and uses the UI font
    // for search (opposite of studio) must drive the matching variables;
    // shadowRadius 0 must disable the shadow.
    const tweaked: ThemeEntry = {
      ...studioTheme,
      metrics: { cardRadius: 14, controlRadius: 6, rowPadding: 16, shadowRadius: 0 },
      typography: {
        sectionTitleSize: 20,
        uppercasesSectionTitles: true,
        searchSize: 12,
        searchUsesDisplayFont: false,
        historyContentSize: 13,
      },
    };
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [tweaked], errors: [] });

    renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("--radius-md")).toBe("14px");
    expect(app!.style.getPropertyValue("--radius-sm")).toBe("6px");
    expect(app!.style.getPropertyValue("--history-row-padding-y")).toBe("16px");
    expect(app!.style.getPropertyValue("--shadow-md")).toBe("none");
    expect(app!.style.getPropertyValue("--font-size-section")).toBe("20px");
    expect(app!.style.getPropertyValue("--search-font-size")).toBe("12px");
    expect(app!.style.getPropertyValue("--search-font-family")).toBe("var(--font-ui)");
    expect(app!.style.getPropertyValue("--section-title-transform")).toBe("uppercase");
  });

  it("clears every injected variable when switching back to a built-in theme", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.length).toBe(39);

    act(() => {
      result.current.setColorTheme("rose");
    });

    expect(app!.style.length).toBe(0);
    expect(app!.style.getPropertyValue("--brand")).toBe("");
    expect(app!.style.getPropertyValue("border-radius")).toBe("");
    expect(app!.style.getPropertyValue("box-shadow")).toBe("");
    // R007: metrics/typography variables are cleared too — no residuals
    // from a custom theme can leak into a built-in theme.
    expect(app!.style.getPropertyValue("--radius-md")).toBe("");
    expect(app!.style.getPropertyValue("--history-row-padding-y")).toBe("");
    expect(app!.style.getPropertyValue("--search-font-size")).toBe("");
    expect(app!.style.getPropertyValue("--section-title-transform")).toBe("");
  });

  it("injects background image and scrim for themes with a background", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });
    getThemeBackgroundMock.mockResolvedValue({
      mimeType: "image/png",
      dataB64: "AAECAwQ=",
    });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(getThemeBackgroundMock).toHaveBeenCalledWith("studio", "light");
    // The data URL is assembled client-side from the validated payload.
    expect(app!.style.getPropertyValue("--art-bg-image")).toBe(
      'url("data:image/png;base64,AAECAwQ=")',
    );
    expect(app!.style.getPropertyValue("--art-bg-scrim")).toBe("rgba(15, 21, 38, 0.82)");
    expect(result.current.resolvedColorTheme).toBe("custom:studio");
  });

  it("does not fetch a background for themes without one", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    const plain: ThemeEntry = { ...studioTheme, background: null };
    listThemesMock.mockResolvedValue({ builtin: [], custom: [plain], errors: [] });

    renderHook(() => useTheme());
    await act(async () => {});

    expect(getThemeBackgroundMock).not.toHaveBeenCalled();
    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("--art-bg-image")).toBe("");
    expect(app!.style.getPropertyValue("--art-bg-scrim")).toBe("");
  });

  it("clears background variables when switching back to a built-in theme", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [studioTheme], errors: [] });
    getThemeBackgroundMock.mockResolvedValue({ mimeType: "image/png", dataB64: "AAECAwQ=" });

    const { result } = renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("--art-bg-image")).not.toBe("");

    act(() => {
      result.current.setColorTheme("rose");
    });

    expect(app!.style.getPropertyValue("--art-bg-image")).toBe("");
    expect(app!.style.getPropertyValue("--art-bg-scrim")).toBe("");
  });

  it("refetches the background for the dark mode and re-injects", async () => {
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    // Both modes carry a background in this scenario, so the mode switch
    // must fetch the matching payload.
    const bothModes: ThemeEntry = {
      ...studioTheme,
      background: {
        light: studioTheme.background!.light,
        dark: {
          hasImage: true,
          scrim: { hex: "#101820", opacity: 0.9 },
          mimeType: "image/jpeg",
        },
      },
    };
    listThemesMock.mockResolvedValue({ builtin: [], custom: [bothModes], errors: [] });
    getThemeBackgroundMock.mockImplementation(
      (_id: string, mode: string) =>
        Promise.resolve({
          mimeType: "image/png",
          dataB64: mode === "dark" ? "DARKBYTES" : "LIGHTBYTES",
        }),
    );

    renderHook(() => useTheme());
    await act(async () => {});

    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("--art-bg-image")).toBe(
      'url("data:image/png;base64,LIGHTBYTES")',
    );

    act(() => mediaQuery.setMatches(true));

    await act(async () => {});
    expect(getThemeBackgroundMock).toHaveBeenCalledWith("studio", "dark");
    expect(app!.style.getPropertyValue("--art-bg-image")).toBe(
      'url("data:image/png;base64,DARKBYTES")',
    );
    // Switching back to light reuses the session cache (no refetch).
    const calls = getThemeBackgroundMock.mock.calls.length;
    act(() => mediaQuery.setMatches(false));
    await act(async () => {});
    expect(getThemeBackgroundMock.mock.calls.length).toBe(calls);
    expect(app!.style.getPropertyValue("--art-bg-image")).toBe(
      'url("data:image/png;base64,LIGHTBYTES")',
    );
  });

  it("warns about unsupported structural keys and shadow:true", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const withUnsupported: ThemeEntry = {
      ...studioTheme,
      structural: {
        borderRadius: 4,
        shadow: true,
        glow: 2,
      } as ThemeEntry["structural"],
    };
    localStorage.setItem("tailsync-color-theme", "custom:studio");
    listThemesMock.mockResolvedValue({ builtin: [], custom: [withUnsupported], errors: [] });

    renderHook(() => useTheme());
    await act(async () => {});

    const messages = warn.mock.calls.map((call) => String(call[0]));
    expect(messages.some((message) => message.includes('structural key "glow"'))).toBe(true);
    expect(messages.some((message) => message.includes("shadow: true"))).toBe(true);
    const app = document.querySelector<HTMLElement>(".app");
    expect(app!.style.getPropertyValue("border-radius")).toBe("4px");
    expect(app!.style.getPropertyValue("box-shadow")).toBe("");
    warn.mockRestore();
  });
});
