import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { isColorTheme, isThemePreference, useTheme } from "./useTheme";

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

describe("useTheme", () => {
  let mediaQuery: MockMediaQueryList;

  beforeEach(() => {
    localStorage.clear();
    mediaQuery = new MockMediaQueryList();
    vi.stubGlobal("matchMedia", vi.fn(() => mediaQuery));
  });

  it("validates persisted theme identifiers", () => {
    expect(isThemePreference("system")).toBe(true);
    expect(isThemePreference("sepia")).toBe(false);
    expect(isColorTheme("high-contrast")).toBe(true);
    expect(isColorTheme("unknown")).toBe(false);
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
});
