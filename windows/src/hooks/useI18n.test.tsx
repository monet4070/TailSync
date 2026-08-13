import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import en from "../i18n/en.json";
import zhCN from "../i18n/zh-CN.json";
import { useI18n } from "./useI18n";

describe("useI18n", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.spyOn(window.navigator, "language", "get").mockReturnValue("en-US");
  });

  it("uses the browser locale and falls back to English keys", () => {
    vi.spyOn(window.navigator, "language", "get").mockReturnValue("zh-CN");

    const { result } = renderHook(() => useI18n());

    expect(result.current.locale).toBe("zh-CN");
    expect(result.current.t("history.title")).toBe(zhCN["history.title"]);
    expect(result.current.t("missing.translation")).toBe("missing.translation");
  });

  it("prefers a stored locale and persists explicit changes", async () => {
    localStorage.setItem("tailsync-lang", "en");
    vi.spyOn(window.navigator, "language", "get").mockReturnValue("zh-CN");
    const { result } = renderHook(() => useI18n());

    expect(result.current.locale).toBe("en");
    expect(result.current.t("history.title")).toBe(en["history.title"]);

    act(() => result.current.setLocale("zh-CN"));

    expect(localStorage.getItem("tailsync-lang")).toBe("zh-CN");
    await waitFor(() => expect(document.documentElement.lang).toBe("zh-CN"));
  });

  it("accepts locale changes from another window", () => {
    const { result } = renderHook(() => useI18n());

    act(() => {
      window.dispatchEvent(new StorageEvent("storage", {
        key: "tailsync-lang",
        newValue: "zh-CN",
      }));
    });

    expect(result.current.locale).toBe("zh-CN");
    expect(result.current.t("history.title")).toBe(zhCN["history.title"]);
  });

  it("falls back to English for an unsupported stored locale", () => {
    localStorage.setItem("tailsync-lang", "fr");
    const { result } = renderHook(() => useI18n());

    expect(result.current.locale).toBe("fr");
    expect(result.current.t("history.title")).toBe(en["history.title"]);
  });

  it("localizes the actionable protocol upgrade prompt", () => {
    expect(en["settings.protocolUpgradeRequired"].replace("{version}", "3"))
      .toContain("protocol v3");
    expect(zhCN["settings.protocolUpgradeRequired"].replace("{version}", "3"))
      .toContain("协议 v3");
  });
});
