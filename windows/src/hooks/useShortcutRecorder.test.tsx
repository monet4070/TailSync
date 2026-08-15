import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useShortcutRecorder } from "./useShortcutRecorder";

vi.mock("../tailsyncClient", () => ({
  resumeSyncShortcut: vi.fn(),
  setSyncShortcut: vi.fn(),
  suspendSyncShortcut: vi.fn(),
}));

vi.mock("../utils/shortcut", () => ({
  captureShortcut: vi.fn(),
}));

vi.mock("./useI18n", () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

import {
  resumeSyncShortcut,
  setSyncShortcut,
  suspendSyncShortcut,
} from "../tailsyncClient";
import { captureShortcut } from "../utils/shortcut";

const mockedResume = vi.mocked(resumeSyncShortcut);
const mockedSet = vi.mocked(setSyncShortcut);
const mockedSuspend = vi.mocked(suspendSyncShortcut);
const mockedCapture = vi.mocked(captureShortcut);

function makeOptions(overrides: Partial<{
  currentShortcut: () => string | null;
  applyShortcut: (s: string) => void;
  showSavedToast: () => void;
  showError: (m: string) => void;
}> = {}) {
  return {
    currentShortcut: () => "Control+Shift+Old",
    applyShortcut: vi.fn(),
    showSavedToast: vi.fn(),
    showError: vi.fn(),
    ...overrides,
  };
}

describe("useShortcutRecorder", () => {
  beforeEach(() => {
    mockedResume.mockReset().mockResolvedValue(undefined);
    mockedSet.mockReset().mockResolvedValue(undefined);
    mockedSuspend.mockReset().mockResolvedValue(undefined);
    mockedCapture.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("commits a shortcut, applies it to the hub, and updates the draft", async () => {
    const opts = makeOptions();
    const { result } = renderHook(() => useShortcutRecorder(opts));

    let ok: boolean;
    await act(async () => {
      ok = await result.current.commitShortcut("Control+Shift+New", true, false);
    });
    expect(ok!).toBe(true);
    expect(mockedSet).toHaveBeenCalledWith("Control+Shift+New");
    expect(opts.applyShortcut).toHaveBeenCalledWith("Control+Shift+New");
    expect(result.current.shortcutDraft).toBe("Control+Shift+New");
    expect(opts.showSavedToast).toHaveBeenCalled();
    expect(opts.showError).toHaveBeenCalledWith("");
  });

  it("rolls back the draft and resumes the shortcut on registration failure", async () => {
    mockedSet.mockRejectedValue(new Error("already registered"));
    const opts = makeOptions();
    const { result } = renderHook(() => useShortcutRecorder(opts));

    let ok: boolean;
    await act(async () => {
      ok = await result.current.commitShortcut("Control+Shift+New", true, false);
    });
    expect(ok!).toBe(false);
    expect(mockedResume).toHaveBeenCalled();
    expect(result.current.shortcutDraft).toBe("Control+Shift+Old");
    expect(opts.showError).toHaveBeenCalledWith("settings.shortcutConflict");
    expect(opts.showSavedToast).not.toHaveBeenCalled();

    // reportInline=true suppresses the global error toast on failure.
    vi.mocked(opts.showError).mockClear();
    mockedSet.mockRejectedValue(new Error("already registered"));
    await act(async () => {
      await result.current.commitShortcut("Control+Shift+New", true, true);
    });
    expect(opts.showError).not.toHaveBeenCalledWith("settings.shortcutConflict");
  });

  it("does nothing when the shortcut is unchanged and registration is not forced", async () => {
    const opts = makeOptions();
    const { result } = renderHook(() => useShortcutRecorder(opts));

    let ok: boolean;
    await act(async () => {
      ok = await result.current.commitShortcut("Control+Shift+Old", false, false);
    });
    expect(ok!).toBe(true);
    expect(mockedSet).not.toHaveBeenCalled();
    expect(opts.applyShortcut).not.toHaveBeenCalled();
    expect(opts.showSavedToast).not.toHaveBeenCalled();
  });

  it("starts recording with the global shortcut suspended, then cancels back", async () => {
    const opts = makeOptions();
    const { result } = renderHook(() => useShortcutRecorder(opts));

    await act(async () => {
      await result.current.startShortcutRecording();
    });
    expect(mockedSuspend).toHaveBeenCalled();
    expect(result.current.shortcutRecording).toBe(true);
    expect(result.current.shortcutCaptureActive).toBe(true);

    await act(async () => {
      await result.current.cancelShortcutRecording();
    });
    expect(mockedResume).toHaveBeenCalled();
    expect(result.current.shortcutRecording).toBe(false);
    expect(result.current.shortcutDraft).toBe("Control+Shift+Old");
  });

  it("captures a key, confirms it, and reports saved inline", async () => {
    mockedCapture.mockReturnValue({
      kind: "shortcut",
      shortcut: "Control+Shift+X",
      keycaps: ["Ctrl", "Shift", "X"],
    });
    const opts = makeOptions();
    const { result } = renderHook(() => useShortcutRecorder(opts));

    await act(async () => {
      await result.current.startShortcutRecording();
    });
    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "x", code: "KeyX", ctrlKey: true, shiftKey: true }),
      );
    });
    expect(mockedCapture).toHaveBeenCalledTimes(1);
    expect(result.current.shortcutCandidate).toBe("Control+Shift+X");
    expect(result.current.shortcutPreviewKeys).toEqual(["Ctrl", "Shift", "X"]);
    expect(result.current.shortcutCaptureActive).toBe(false);

    await act(async () => {
      await result.current.confirmShortcut();
    });
    expect(mockedSet).toHaveBeenCalledWith("Control+Shift+X");
    expect(opts.applyShortcut).toHaveBeenCalledWith("Control+Shift+X");
    expect(opts.showSavedToast).toHaveBeenCalled();
    expect(result.current.shortcutRecording).toBe(false);
  });

  it("restores the shortcut on unmount while recording", async () => {
    const opts = makeOptions();
    const { result, unmount } = renderHook(() => useShortcutRecorder(opts));

    await act(async () => {
      await result.current.startShortcutRecording();
    });
    expect(mockedResume).not.toHaveBeenCalled();

    await act(async () => {
      await unmount();
    });
    expect(mockedResume).toHaveBeenCalled();
  });
});
