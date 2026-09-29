import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import * as client from "../tailsyncClient";
import type { SettingsData } from "../types/settings.generated";
import { useSettingsEditor } from "./useSettingsEditor";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../tailsyncClient", async (importOriginal) => ({
  ...await importOriginal<typeof client>(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

const initial: SettingsData = {
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
  sync_shortcut: "Alt+V",
  trusted_peer_addresses: {},
  trusted_peer_keys: {},
};

describe("settings windows", () => {
  beforeEach(() => vi.clearAllMocks());

  it("preserves two windows' independent edits and refreshes both copies", async () => {
    let persisted = { ...initial };
    const listeners: Array<() => void> = [];
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      const callback = () => handler({ event: "settings-changed", id: 0, payload: null });
      listeners.push(callback);
      return () => { listeners.splice(listeners.indexOf(callback), 1); };
    });
    vi.mocked(client.getSettings).mockImplementation(async () => ({ ...persisted }));
    vi.mocked(client.updateSettings).mockImplementation(async (patch) => {
      persisted = { ...persisted, ...patch };
      return { ...persisted };
    });

    const setLocale = vi.fn();
    const connections = renderHook(() => useSettingsEditor(setLocale, "save failed"));
    const settings = renderHook(() => useSettingsEditor(setLocale, "save failed"));
    act(() => {
      connections.result.current.applyCanonical(initial);
      settings.result.current.applyCanonical(initial);
    });

    await act(async () => {
      await connections.result.current.update({ connection_mode: "lan_only" });
      await settings.result.current.update({ language: "zh-CN" });
    });
    expect(client.updateSettings).toHaveBeenNthCalledWith(1, { connection_mode: "lan_only" });
    expect(client.updateSettings).toHaveBeenNthCalledWith(2, { language: "zh-CN" });
    expect(persisted).toMatchObject({ connection_mode: "lan_only", language: "zh-CN" });

    await act(async () => { listeners.forEach((notify) => notify()); });
    await waitFor(() => {
      expect(connections.result.current.settings).toMatchObject(persisted);
      expect(settings.result.current.settings).toMatchObject(persisted);
    });
  });
});
