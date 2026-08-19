import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useTheme } from "./useTheme";

const state = vi.hoisted(() => ({ get: vi.fn(), set: vi.fn(), resolve: vi.fn(), handlers: new Map<string, (event: { payload: any }) => void>() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn((name: string, handler: (event: { payload: any }) => void) => { state.handlers.set(name, handler); return Promise.resolve(vi.fn()); }) }));
vi.mock("../tailsyncClient", () => ({ getLocalThemeSettingsV2: state.get, setLocalThemeSettingsV2: state.set, resolveThemeV2: state.resolve }));

describe("useTheme V2 local settings", () => {
  beforeEach(() => {
    Object.defineProperty(window, "matchMedia", { configurable: true, value: () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }) });
    localStorage.clear(); state.handlers.clear(); state.set.mockReset();
    state.get.mockResolvedValue({ activeThemeId: "custom:midnight", appearance: "dark", highContrast: false });
    state.resolve.mockResolvedValue({ tokens: { colors: { background: { canvas: "#111111", surface: "#222222" }, text: { primary: "#fff", secondary: "#aaa" }, accent: { default: "#f00" } } } });
    document.body.innerHTML = '<div id="root"><div class="app"></div></div>';
  });

  it("starts from Core local selection, never localStorage or synced settings", async () => {
    localStorage.setItem("tailsync-color-theme", "rose");
    const { result } = renderHook(() => useTheme());
    await waitFor(() => expect(result.current.colorTheme).toBe("custom:midnight"));
    expect(result.current.themePreference).toBe("dark");
    expect(state.resolve).toHaveBeenCalledWith("custom:midnight", "dark");
  });

  it("applies theme_changed in every open window", async () => {
    const { result } = renderHook(() => useTheme());
    await waitFor(() => expect(state.handlers.get("theme_changed")).toBeDefined());
    act(() => state.handlers.get("theme_changed")!({ payload: { activeThemeId: "builtin:canvas@1", appearance: "light", highContrast: false } }));
    expect(result.current.colorTheme).toBe("builtin:canvas@1");
    expect(result.current.themePreference).toBe("light");
  });

  it("converges failed resolution to persisted Canvas", async () => {
    state.resolve.mockRejectedValueOnce(new Error("bad package"));
    const { result } = renderHook(() => useTheme());
    await waitFor(() => expect(result.current.colorTheme).toBe("builtin:canvas@1"));
    expect(state.set).toHaveBeenCalledWith(expect.objectContaining({ activeThemeId: "builtin:canvas@1" }));
  });

  it("passes a persisted high-contrast preference to the Core resolver", async () => {
    state.get.mockResolvedValueOnce({ activeThemeId: "custom:midnight", appearance: "dark", highContrast: true });
    renderHook(() => useTheme());
    await waitFor(() => expect(state.resolve).toHaveBeenCalledWith("custom:midnight", "dark", true));
  });

  it("updates the history font variable when the active theme changes", async () => {
    state.get.mockResolvedValue({ activeThemeId: "builtin:canvas@1", appearance: "light", highContrast: false });
    state.resolve.mockImplementation((themeId: string) => Promise.resolve({
      tokens: {
        typography: {
          display: { families: [themeId === "builtin:flux@1" ? "Flux Display" : "Canvas Display"] },
          reading: { families: ["Reading"] },
        },
      },
    }));

    renderHook(() => useTheme());
    await waitFor(() => expect(document.querySelector<HTMLElement>(".app")?.style.getPropertyValue("--font-history"))
      .toBe("var(--font-display)"));

    act(() => state.handlers.get("theme_changed")!({
      payload: { activeThemeId: "builtin:flux@1", appearance: "light", highContrast: false },
    }));
    await waitFor(() => expect(document.querySelector<HTMLElement>(".app")?.style.getPropertyValue("--font-history"))
      .toBe("var(--font-content)"));
  });

  it("keeps the transparent document backing surface across theme changes", async () => {
    state.resolve.mockImplementation((themeId: string) => Promise.resolve({
      tokens: { colors: { background: { canvas: themeId === "builtin:canvas@1" ? "#faf4f8" : "#111111" } } },
    }));

    renderHook(() => useTheme());
    await waitFor(() => expect(document.body.style.backgroundColor).toBe("transparent"));
    expect(document.documentElement.style.backgroundColor).toBe("transparent");
    expect(document.getElementById("root")?.style.backgroundColor).toBe("transparent");
    expect(document.querySelector<HTMLElement>(".app")?.style.getPropertyValue("--bg-window")).toBe("#111111");

    act(() => state.handlers.get("theme_changed")!({
      payload: { activeThemeId: "builtin:canvas@1", appearance: "light", highContrast: false },
    }));
    await waitFor(() => expect(document.body.style.backgroundColor).toBe("transparent"));
    expect(document.documentElement.style.backgroundColor).toBe("transparent");
    expect(document.getElementById("root")?.style.backgroundColor).toBe("transparent");
    expect(document.querySelector<HTMLElement>(".app")?.style.getPropertyValue("--bg-window")).toBe("#faf4f8");
  });
});
