export type ShortcutKeyboardEvent = Pick<
  KeyboardEvent,
  "code" | "key" | "location" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey"
>;

export type ShortcutCaptureResult =
  | { kind: "modifier"; keycaps: string[] }
  | { kind: "shortcut"; shortcut: string; keycaps: string[] }
  | { kind: "invalid"; reason: "modifier-required" | "unsupported"; keycaps: string[] };

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

const MODIFIER_KEYS = new Set(["Control", "Meta", "Alt", "Shift", "AltGraph"]);

const SUPPORTED_CODES = new Set([
  "Backquote",
  "Backslash",
  "BracketLeft",
  "BracketRight",
  "Pause",
  "Comma",
  "Equal",
  "Minus",
  "Period",
  "Quote",
  "Semicolon",
  "Slash",
  "Backspace",
  "CapsLock",
  "Enter",
  "Space",
  "Tab",
  "Delete",
  "End",
  "Home",
  "Insert",
  "PageDown",
  "PageUp",
  "PrintScreen",
  "ScrollLock",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "NumLock",
  "NumpadAdd",
  "NumpadDecimal",
  "NumpadDivide",
  "NumpadEnter",
  "NumpadEqual",
  "NumpadMultiply",
  "NumpadSubtract",
  "Escape",
  "AudioVolumeDown",
  "AudioVolumeUp",
  "AudioVolumeMute",
  "MediaPlay",
  "MediaPause",
  "MediaPlayPause",
  "MediaStop",
  "MediaTrackNext",
  "MediaTrackPrevious",
]);

const KEYCAP_LABELS: Record<string, string> = {
  CommandOrControl: "Ctrl",
  Control: "Ctrl",
  Command: "Cmd",
  Super: "Win",
  Alt: "Alt",
  Shift: "Shift",
  Space: "Space",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
  Backquote: "`",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Equal: "=",
  Minus: "-",
  Period: ".",
  Quote: "'",
  Semicolon: ";",
  Slash: "/",
  NumpadAdd: "Num +",
  NumpadDecimal: "Num .",
  NumpadDivide: "Num /",
  NumpadEnter: "Num Enter",
  NumpadEqual: "Num =",
  NumpadMultiply: "Num *",
  NumpadSubtract: "Num -",
  AudioVolumeDown: "Volume -",
  AudioVolumeUp: "Volume +",
  AudioVolumeMute: "Mute",
  MediaPlay: "Play",
  MediaPause: "Pause",
  MediaPlayPause: "Play/Pause",
  MediaStop: "Stop",
  MediaTrackNext: "Next",
  MediaTrackPrevious: "Previous",
};

function supportedCode(code: string) {
  if (SUPPORTED_CODES.has(code)) return code;
  if (/^Key[A-Z]$/.test(code)) return code;
  if (/^Digit[0-9]$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return null;
}

function fallbackCode(key: string, location: number) {
  if (location === KeyboardEvent.DOM_KEY_LOCATION_NUMPAD) {
    if (/^[0-9]$/.test(key)) return `Numpad${key}`;
    const numpadKeys: Record<string, string> = {
      "+": "NumpadAdd",
      ".": "NumpadDecimal",
      "/": "NumpadDivide",
      "=": "NumpadEqual",
      "*": "NumpadMultiply",
      "-": "NumpadSubtract",
      Enter: "NumpadEnter",
    };
    if (numpadKeys[key]) return numpadKeys[key];
  }

  if (/^[a-z]$/i.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/i.test(key)) return key.toUpperCase();
  const aliases: Record<string, string> = {
    " ": "Space",
    "`": "Backquote",
    "~": "Backquote",
    "\\": "Backslash",
    "|": "Backslash",
    "[": "BracketLeft",
    "{": "BracketLeft",
    "]": "BracketRight",
    "}": "BracketRight",
    ",": "Comma",
    "<": "Comma",
    "=": "Equal",
    "+": "Equal",
    "-": "Minus",
    "_": "Minus",
    ".": "Period",
    ">": "Period",
    "'": "Quote",
    "\"": "Quote",
    ";": "Semicolon",
    ":": "Semicolon",
    "/": "Slash",
    "?": "Slash",
    Esc: "Escape",
  };
  return supportedCode(aliases[key] ?? key);
}

function shortcutModifiers(event: ShortcutKeyboardEvent) {
  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Control");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");
  return modifiers;
}

function keycapLabel(part: string) {
  if (KEYCAP_LABELS[part]) return KEYCAP_LABELS[part];
  if (/^Key[A-Z]$/.test(part)) return part.slice(3);
  if (/^Digit[0-9]$/.test(part)) return part.slice(5);
  if (/^Numpad[0-9]$/.test(part)) return `Num ${part.slice(6)}`;
  return part;
}

export function shortcutKeycaps(shortcut: string) {
  if (!shortcut) return [];
  return shortcut.split("+").map(keycapLabel);
}

export function captureShortcut(event: ShortcutKeyboardEvent): ShortcutCaptureResult {
  const modifiers = shortcutModifiers(event);
  const modifierKeycaps = modifiers.map(keycapLabel);
  if (MODIFIER_CODES.has(event.code) || MODIFIER_KEYS.has(event.key)) {
    return { kind: "modifier", keycaps: modifierKeycaps };
  }

  const code = supportedCode(event.code) ?? fallbackCode(event.key, event.location);
  if (!code) {
    return {
      kind: "invalid",
      reason: "unsupported",
      keycaps: [...modifierKeycaps, event.key].filter(Boolean),
    };
  }
  if (modifiers.length === 0) {
    return {
      kind: "invalid",
      reason: "modifier-required",
      keycaps: [keycapLabel(code)],
    };
  }

  const shortcut = [...modifiers, code].join("+");
  return { kind: "shortcut", shortcut, keycaps: shortcutKeycaps(shortcut) };
}
