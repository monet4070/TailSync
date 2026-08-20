import { describe, expect, it } from "vitest";
import { historyFontRole, themeV2CssPairs } from "./themeV2Css";

describe("themeV2CssPairs", () => {
  it("maps every semantic V2 colour and structural token to the CSS contract", () => {
    const tokens = { colors: { accent: { default: "a", hover: "b", soft: "c", onAccent: "d" }, background: { canvas: "e", surface: "f", input: "g", hover: "h", active: "i", raised: "j", toast: "k" }, text: { primary: "l", secondary: "m", tertiary: "n", toast: "o" }, border: { default: "p", strong: "q", divider: "r" }, status: { positive: "s", positiveSoft: "t", warning: "u", warningSoft: "v", info: "w", infoSoft: "x" } }, typography: { ui: { families: ["UI", "sans"], size: 14, lineHeight: 20, weight: 500 }, display: { families: ["Display"] }, reading: { families: ["Reading"] }, search: { size: 17, useDisplayFont: true }, section: { size: 12, uppercase: false }, history: { size: 15 } }, density: { control: 8, row: 12 }, shape: { controlRadius: 3, surfaceRadius: 5, windowRadius: 7 }, effects: { shadow: { radius: 9, y: 2, opacity: .3 }, motion: { fast: 100, slow: 200, easing: "linear" } } };
    const css = Object.fromEntries(themeV2CssPairs(tokens));
    expect(css).toMatchObject({ "--brand": "a", "--brand-hover": "b", "--brand-soft": "c", "--brand-text": "d", "--bg-window": "e", "--bg-card": "f", "--bg-input": "g", "--bg-hover": "h", "--bg-active": "i", "--bg-raised": "j", "--bg-toast": "k", "--text-primary": "l", "--text-secondary": "m", "--text-tertiary": "n", "--text-toast": "o", "--border": "p", "--border-strong": "q", "--divider": "r", "--green": "s", "--green-soft": "t", "--orange": "u", "--orange-soft": "v", "--purple": "w", "--purple-soft": "x", "--font-ui": "UI, sans", "--font-display": "Display", "--font-content": "Reading", "--font-history": "var(--font-display)", "--search-font-family": "var(--font-display)", "--shadow-md": "0 2px 9px rgba(0, 0, 0, 0.3)", "--art-fast": "100ms", "--art-slow": "200ms", "--art-spring": "linear" });
  });

  it("selects the history face using the cross-platform theme semantics", () => {
    expect(historyFontRole("builtin:canvas@1")).toBe("display");
    expect(historyFontRole("builtin:ledger@1")).toBe("display");
    expect(historyFontRole("custom:tailsync.aura-bloom")).toBe("display");
    expect(historyFontRole("builtin:flux@1")).toBe("reading");
    expect(historyFontRole("builtin:aura@1")).toBe("reading");
    expect(historyFontRole("builtin:mono@1")).toBe("reading");

    const tokens = { typography: { display: { families: ["Display"] }, reading: { families: ["Reading"] } } };
    expect(Object.fromEntries(themeV2CssPairs(tokens, { themeId: "builtin:canvas@1" }))["--font-history"])
      .toBe("var(--font-display)");
    expect(Object.fromEntries(themeV2CssPairs(tokens, { themeId: "builtin:flux@1" }))["--font-history"])
      .toBe("var(--font-content)");
  });

  it("maps resolved component states without exposing renderer properties", () => {
    const css = Object.fromEntries(themeV2CssPairs({ components: {
      search: { focus: { background: "#123456", foreground: "#ffffff", focusRing: "#abcdef", padding: 12, typography: { size: 16, weight: 600 }, shadow: { radius: 4, y: 2, opacity: .2 } } },
      button: { hover: { accent: "#345678", radius: 4, spacing: 6 } },
    } }));
    expect(css).toMatchObject({
      "--theme-search-focus-background": "#123456",
      "--theme-search-focus-foreground": "#ffffff",
      "--theme-search-focus-focus-ring": "#abcdef",
      "--theme-search-focus-padding": "12px",
      "--theme-search-focus-font-size": "16px",
      "--theme-search-focus-font-weight": "600",
      "--theme-search-focus-shadow": "0 2px 4px rgba(0, 0, 0, 0.2)",
      "--theme-button-hover-accent": "#345678",
      "--theme-button-hover-radius": "4px",
      "--theme-button-hover-spacing": "6px",
    });
  });

  it("makes palette and component colors opaque and removes shadows for reduced transparency", () => {
    const css = Object.fromEntries(themeV2CssPairs({
      colors: {
        background: { canvas: "rgba(255, 255, 255, 0.8)", surface: "rgba(0, 0, 0, 0.5)" },
        text: { primary: "#11223380" },
      },
      effects: { shadow: { radius: 8, y: 2, opacity: 0.4 } },
      components: { panel: { default: {
        background: "rgba(10, 20, 30, 0.25)",
        shadow: { radius: 4, y: 1, opacity: 0.2 },
      } } },
    }, { reduceTransparency: true, mode: "light" }));

    expect(css["--bg-window"]).toBe("rgb(255, 255, 255)");
    expect(css["--bg-card"]).toBe("rgb(128, 128, 128)");
    expect(css["--text-primary"]).toBe("rgb(136, 144, 153)");
    expect(css["--theme-panel-default-background"]).toBe("rgb(194, 196, 199)");
    expect(css["--shadow-md"]).toBe("none");
    expect(css["--theme-panel-default-shadow"]).toBe("none");
  });
});
