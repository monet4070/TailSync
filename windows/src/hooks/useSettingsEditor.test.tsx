import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../types/settings.generated";

const { listenMock } = vi.hoisted(() => ({ listenMock: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

vi.mock("../tailsyncClient", () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

import { getSettings, updateSettings } from "../tailsyncClient";
import { useSettingsEditor } from "./useSettingsEditor";

const mockedGetSettings = vi.mocked(getSettings);
const mockedUpdateSettings = vi.mocked(updateSettings);

const baseSettings: SettingsData = {
  connection_mode: "auto",
  enabled_peers: {},
  history_limit: 100,
  history_shortcut: "Alt+H",
  language: "en",
  notifications_enabled: true,
  paired_peer_endpoints: {},
  progress_bar_enabled: true,
  storage_quota_bytes: 10737418240,
  storage_root: null,
  sync_enabled: true,
  sync_shortcut: "Alt+S",
  trusted_peer_addresses: {},
  trusted_peer_keys: {},
  auto_save_network_images: false,
};

/** The daemon answers a patch with the state it committed. */
function canonical(server: SettingsData, patch: Partial<SettingsData>): SettingsData {
  return { ...server, ...patch };
}

const settingsChangedHandlers = new Map<string, () => void>();
const unlisteners: Array<() => void> = [];
let serverState: SettingsData = { ...baseSettings };

describe("useSettingsEditor", () => {
  beforeEach(() => {
    settingsChangedHandlers.clear();
    unlisteners.length = 0;
    listenMock.mockReset();
    listenMock.mockImplementation(async (name: string, handler: () => void) => {
      settingsChangedHandlers.set(name, handler);
      const stop = vi.fn(() => settingsChangedHandlers.delete(name));
      unlisteners.push(stop);
      return stop;
    });
    mockedGetSettings.mockReset();
    mockedUpdateSettings.mockReset();
    serverState = { ...baseSettings };
    mockedGetSettings.mockResolvedValue(baseSettings);
    mockedUpdateSettings.mockImplementation(async (patch) => {
      serverState = canonical(serverState, patch);
      return serverState;
    });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  it("sends only the fields each window changed and installs the canonical answer", async () => {
    const windowA = renderHook(() => useSettingsEditor());
    const windowB = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(windowA.result.current.settings).not.toBeNull());
    await waitFor(() => expect(windowB.result.current.settings).not.toBeNull());

    await act(async () => {
      await windowA.result.current.update({ history_limit: 250 });
    });
    // The second window read the same starting snapshot, but its own save only
    // carries the field it changed, so the first window's edit survives.
    await act(async () => {
      await windowB.result.current.update({ connection_mode: "lan_only" });
    });

    expect(mockedUpdateSettings).toHaveBeenNthCalledWith(1, { history_limit: 250 });
    expect(mockedUpdateSettings).toHaveBeenNthCalledWith(2, { connection_mode: "lan_only" });
    expect(windowB.result.current.settings?.history_limit).toBe(250);
    expect(windowB.result.current.settings?.connection_mode).toBe("lan_only");
    expect(windowA.result.current.settings?.connection_mode).toBe("auto");
  });

  it("keeps a newer draft when an older successful response arrives late", async () => {
    let release: (value: SettingsData) => void = () => undefined;
    mockedUpdateSettings.mockImplementationOnce(
      () => new Promise<SettingsData>((resolve) => { release = resolve; }),
    );
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());

    let save: Promise<boolean> = Promise.resolve(false);
    act(() => {
      save = result.current.update({ history_limit: 250 });
    });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledTimes(1));
    // A dedicated command edits the draft while the generic save is in flight.
    act(() => {
      result.current.installLocal({ sync_shortcut: "Alt+Z" });
    });

    await act(async () => {
      release(canonical(baseSettings, { history_limit: 250 }));
      await save;
    });

    expect(result.current.settings?.sync_shortcut).toBe("Alt+Z");
    expect(result.current.settings?.history_limit).toBe(250);
  });

  it("keeps a newer draft when the failure refetch resolves after the edit", async () => {
    let releaseRefetch: (value: SettingsData) => void = () => undefined;
    mockedGetSettings
      .mockResolvedValueOnce(baseSettings)
      .mockImplementationOnce(
        () => new Promise<SettingsData>((resolve) => { releaseRefetch = resolve; }),
      );
    mockedUpdateSettings.mockRejectedValue(new Error("save failed"));
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());

    let save: Promise<boolean> = Promise.resolve(true);
    act(() => {
      save = result.current.update({ history_limit: 250 });
    });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(mockedGetSettings).toHaveBeenCalledTimes(2));

    act(() => {
      result.current.installLocal({ notifications_enabled: false });
    });
    await act(async () => {
      releaseRefetch(baseSettings);
      await save;
    });

    expect(
      result.current.settings?.notifications_enabled,
      "the refetched snapshot must not overwrite the newer draft",
    ).toBe(false);
  });

  it("rolls a failed save back to the canonical snapshot when nothing else changed", async () => {
    mockedUpdateSettings.mockRejectedValue(new Error("save failed"));
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());

    let saved = true;
    await act(async () => {
      saved = await result.current.update({ history_limit: 250 });
    });

    expect(saved).toBe(false);
    expect(result.current.settings?.history_limit).toBe(100);
    expect(result.current.errorMessage).toBe("settings.saveFailed");
  });

  it("converges on another window's canonical snapshot only while idle", async () => {
    let release: (value: SettingsData) => void = () => undefined;
    mockedUpdateSettings.mockImplementationOnce(
      () => new Promise<SettingsData>((resolve) => { release = resolve; }),
    );
    mockedGetSettings
      .mockResolvedValueOnce(baseSettings)
      // The notification is deferred until the save settles.
      .mockResolvedValue({ ...baseSettings, history_limit: 250, language: "zh-CN" });
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    await waitFor(() => expect(settingsChangedHandlers.has("settings-changed")).toBe(true));

    let save: Promise<boolean> = Promise.resolve(false);
    act(() => {
      save = result.current.update({ history_limit: 250 });
    });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledTimes(1));
    // The event arrives while this window still owns an in-flight save.
    await act(async () => {
      settingsChangedHandlers.get("settings-changed")?.();
    });
    expect(result.current.settings?.language).toBe("en");

    await act(async () => {
      release(canonical(baseSettings, { history_limit: 250 }));
      await save;
    });

    await waitFor(() => expect(result.current.settings?.language).toBe("zh-CN"));
    // Idle notifications also refresh the canonical snapshot.
    await act(async () => {
      settingsChangedHandlers.get("settings-changed")?.();
    });
    expect(result.current.settings?.language).toBe("zh-CN");
  });

  it("reports the canonical locale from the hook instead of the window", async () => {
    const onLocale = vi.fn();
    renderHook(() => useSettingsEditor({ onLocale }));
    await waitFor(() => expect(onLocale).toHaveBeenCalledWith("en"));
  });

  it("removes its event listener and stops installing after unmount", async () => {
    let release: (value: SettingsData) => void = () => undefined;
    mockedUpdateSettings.mockImplementationOnce(
      () => new Promise<SettingsData>((resolve) => { release = resolve; }),
    );
    const { result, unmount } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    await waitFor(() => expect(unlisteners.length).toBe(1));

    let save: Promise<boolean> = Promise.resolve(false);
    act(() => {
      save = result.current.update({ history_limit: 250 });
    });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledTimes(1));
    unmount();
    expect(unlisteners[0]).toHaveBeenCalled();

    await act(async () => {
      release(canonical(baseSettings, { history_limit: 250 }));
      await expect(save).resolves.toBe(true);
    });
  });
  it("rejects a StrictMode hydration response arriving after a local save", async () => {
    let releaseOld: (value: SettingsData) => void = () => undefined;
    mockedGetSettings.mockImplementationOnce(() => new Promise(resolve => { releaseOld = resolve; }))
      .mockResolvedValueOnce(baseSettings);
    const { result } = renderHook(() => useSettingsEditor(), { reactStrictMode: true });
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    await act(async () => { await result.current.update({ history_limit: 250 }); });
    await act(async () => { releaseOld(baseSettings); });
    expect(result.current.settings?.history_limit).toBe(250);
  });

  it("does not install a refresh started during a save after that save settles", async () => {
    let releaseSave: (value: SettingsData) => void = () => undefined;
    let releaseRead: (value: SettingsData) => void = () => undefined;
    mockedUpdateSettings.mockImplementationOnce(() => new Promise(resolve => { releaseSave = resolve; }));
    mockedGetSettings.mockResolvedValueOnce(baseSettings)
      .mockImplementationOnce(() => new Promise(resolve => { releaseRead = resolve; }));
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    let save: Promise<boolean>;
    act(() => { save = result.current.update({ history_limit: 250 }); });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledOnce());
    act(() => { settingsChangedHandlers.get("settings-changed")?.(); });
    expect(mockedGetSettings).toHaveBeenCalledTimes(1);
    await act(async () => { releaseSave({ ...baseSettings, history_limit: 250 }); await save!; });
    // The first event's read was taken before the commit. Once deferred to
    // after the commit, the daemon returns the new canonical snapshot instead.
    await act(async () => { releaseRead({ ...baseSettings, history_limit: 250 }); });
    expect(result.current.settings?.history_limit).toBe(250);
  });

  it("invalidates an idle refresh when a newer save starts and completes", async () => {
    let releaseOld: (value: SettingsData) => void = () => undefined;
    mockedGetSettings.mockResolvedValueOnce(baseSettings)
      .mockImplementationOnce(() => new Promise(resolve => { releaseOld = resolve; }));
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    act(() => { settingsChangedHandlers.get("settings-changed")?.(); });
    await act(async () => { await result.current.update({ history_limit: 250 }); });
    await act(async () => { releaseOld(baseSettings); });
    expect(result.current.settings?.history_limit).toBe(250);
  });

  it("converges on an event received during a save without requiring a second event", async () => {
    let release: (value: SettingsData) => void = () => undefined;
    mockedUpdateSettings.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    mockedGetSettings.mockResolvedValueOnce(baseSettings)
      .mockResolvedValue({ ...baseSettings, history_limit: 250, language: "zh-CN" });
    const { result } = renderHook(() => useSettingsEditor());
    await waitFor(() => expect(result.current.settings).not.toBeNull());
    let save: Promise<boolean>;
    act(() => { save = result.current.update({ history_limit: 250 }); });
    await waitFor(() => expect(mockedUpdateSettings).toHaveBeenCalledOnce());
    await act(async () => { settingsChangedHandlers.get("settings-changed")?.(); });
    await act(async () => { release({ ...baseSettings, history_limit: 250 }); await save!; });
    await waitFor(() => expect(result.current.settings?.language).toBe("zh-CN"));
  });

});
