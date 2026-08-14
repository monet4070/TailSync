import { describe, expect, it } from "vitest";
import { captureShortcut, shortcutKeycaps, type ShortcutKeyboardEvent } from "./shortcut";

function keyboardEvent(overrides: Partial<ShortcutKeyboardEvent>): ShortcutKeyboardEvent {
  return {
    code: "KeyK",
    key: "k",
    location: KeyboardEvent.DOM_KEY_LOCATION_STANDARD,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides,
  };
}

describe("shortcut capture", () => {
  it("stores physical key codes instead of layout-dependent characters", () => {
    expect(captureShortcut(keyboardEvent({
      code: "Slash",
      key: "?",
      ctrlKey: true,
      shiftKey: true,
    }))).toEqual({
      kind: "shortcut",
      shortcut: "Control+Shift+Slash",
      keycaps: ["Ctrl", "Shift", "/"],
    });
  });

  it.each([
    ["F24", "F24", "Control+F24"],
    ["ArrowLeft", "ArrowLeft", "Control+ArrowLeft"],
    ["NumpadAdd", "+", "Control+NumpadAdd"],
    ["Delete", "Delete", "Control+Delete"],
    ["Digit8", "*", "Control+Digit8"],
  ])("supports %s", (code, key, shortcut) => {
    const result = captureShortcut(keyboardEvent({ code, key, ctrlKey: true }));
    expect(result.kind).toBe("shortcut");
    if (result.kind === "shortcut") expect(result.shortcut).toBe(shortcut);
  });

  it("keeps Ctrl and Win distinct", () => {
    expect(captureShortcut(keyboardEvent({ ctrlKey: true, metaKey: true }))).toMatchObject({
      kind: "shortcut",
      shortcut: "Control+Super+KeyK",
    });
  });

  it("falls back to key and location when a WebView omits the physical code", () => {
    expect(captureShortcut(keyboardEvent({
      code: "",
      key: "?",
      ctrlKey: true,
      shiftKey: true,
    }))).toMatchObject({ kind: "shortcut", shortcut: "Control+Shift+Slash" });
    expect(captureShortcut(keyboardEvent({
      code: "Unidentified",
      key: "+",
      location: KeyboardEvent.DOM_KEY_LOCATION_NUMPAD,
      ctrlKey: true,
    }))).toMatchObject({ kind: "shortcut", shortcut: "Control+NumpadAdd" });
    expect(captureShortcut(keyboardEvent({
      code: "",
      key: "Shift",
      shiftKey: true,
    }))).toEqual({ kind: "modifier", keycaps: ["Shift"] });
  });

  it("requires a modifier for a global shortcut", () => {
    expect(captureShortcut(keyboardEvent({ code: "KeyQ", key: "q" }))).toEqual({
      kind: "invalid",
      reason: "modifier-required",
      keycaps: ["Q"],
    });
  });

  it("formats stored shortcuts as compact keycaps", () => {
    expect(shortcutKeycaps("CommandOrControl+Shift+KeyS")).toEqual(["Ctrl", "Shift", "S"]);
    expect(shortcutKeycaps("Control+Alt+Numpad1")).toEqual(["Ctrl", "Alt", "Num 1"]);
  });
});
