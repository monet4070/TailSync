import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../types/settings.generated";
import { Settings } from "./Settings";

const {
  hideMock,
  invokeMock,
  listenMock,
  eventHandlers,
  openMock,
  setColorThemeMock,
  setLocaleMock,
  setThemeMock,
} = vi.hoisted(() => ({
  hideMock: vi.fn(),
  invokeMock: vi.fn(),
  eventHandlers: new Map<string, (event: { payload: unknown }) => void>(),
  listenMock: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    eventHandlers.set(event, handler);
    return Promise.resolve(vi.fn());
  }),
  openMock: vi.fn(),
  setColorThemeMock: vi.fn(),
  setLocaleMock: vi.fn(),
  setThemeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: hideMock }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openMock }));
vi.mock("../hooks/useTheme", () => ({
  COLOR_THEMES: ["tailsync", "ocean", "forest", "rose", "high-contrast"],
  isColorTheme: () => true,
  isThemePreference: () => true,
  useTheme: () => ({
    theme: "light",
    themePreference: "system",
    colorTheme: "tailsync",
    setTheme: setThemeMock,
    setColorTheme: setColorThemeMock,
  }),
}));
vi.mock("../hooks/useI18n", () => ({
  useI18n: () => ({
    t: (key: string) => key,
    setLocale: setLocaleMock,
  }),
}));

const settings: SettingsData = {
  color_theme: "tailsync",
  connection_mode: "auto",
  enabled_peers: {},
  history_limit: 100,
  language: "en",
  notifications_enabled: true,
  paired_peer_endpoints: {},
  progress_bar_enabled: true,
  storage_quota_bytes: 10 * 1024 * 1024 * 1024,
  storage_root: null,
  sync_enabled: true,
  sync_shortcut: "CommandOrControl+Shift+S",
  theme: "system",
  trusted_peer_addresses: {},
  trusted_peer_keys: {},
};

const peers = {
  self: {
    hostname: "test-pc",
    tailscale_ip: "",
    connection_mode: "auto",
    public_key: "public-key",
    fingerprint: "fingerprint",
    iroh_endpoint_id: null,
  },
  peers: [],
  paired_peer_endpoints: {},
};

function installInvokeMock(updatesEnabled: boolean) {
  invokeMock.mockImplementation((command: string) => {
    switch (command) {
      case "get_settings":
        return Promise.resolve(settings);
      case "get_storage_status":
        return Promise.resolve({
          root: "C:\\TailSync",
          used_bytes: 0,
          quota_bytes: settings.storage_quota_bytes,
          available: true,
        });
      case "get_update_status":
        return Promise.resolve({
          current_version: "2.1.0",
          updates_enabled: updatesEnabled,
        });
      case "get_peers":
        return Promise.resolve(peers);
      case "check_for_update":
        return Promise.resolve({
          current_version: "2.1.0",
          version: "2.2.0",
          notes: "Release notes",
          published_at: null,
        });
      case "install_update":
        return Promise.resolve(true);
      default:
        return Promise.resolve(undefined);
    }
  });
}

describe("Settings updates", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventHandlers.clear();
  });

  it("shows the app version without contacting the update service when no key is configured", async () => {
    installInvokeMock(false);
    render(<Settings />);

    expect(await screen.findByText("TailSync 2.1.0")).toBeInTheDocument();
    expect(screen.getByText("settings.updateDisabled")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "settings.updateCheck" }));
    expect(invokeMock).not.toHaveBeenCalledWith("check_for_update");
  });

  it("checks for and installs a signed update from the settings footer", async () => {
    installInvokeMock(true);
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.updateCheck" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("check_for_update");
    });

    const installButton = await screen.findByRole("button", { name: "settings.updateInstall" });
    expect(screen.getByText("settings.updateAvailable")).toBeInTheDocument();
    fireEvent.click(installButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("install_update");
      expect(screen.getByText("settings.updateInstalled")).toBeInTheDocument();
    });
  });

  it("reflects tray and global-shortcut sync changes in the visible toggle", async () => {
    installInvokeMock(false);
    render(<Settings />);

    const label = await screen.findByText("settings.syncEnabled");
    const toggle = label.closest(".setting-row")?.querySelector("input[type='checkbox']");
    expect(toggle).toBeChecked();

    await waitFor(() => expect(eventHandlers.has("sync-state-changed")).toBe(true));
    act(() => {
      eventHandlers.get("sync-state-changed")?.({ payload: { enabled: false } });
    });

    expect(toggle).not.toBeChecked();
  });

  it("records a physical-key shortcut and saves it only after confirmation", async () => {
    installInvokeMock(false);
    render(<Settings />);

    const recorder = await screen.findByRole("button", { name: "settings.shortcutRecord" });
    fireEvent.click(recorder);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("suspend_sync_shortcut"));
    const captureTarget = screen.getByRole("button", { name: "settings.shortcutRecording" });

    fireEvent.keyDown(captureTarget, {
      code: "Slash",
      key: "?",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(invokeMock).not.toHaveBeenCalledWith("set_sync_shortcut", expect.anything());
    expect(screen.getByText("settings.shortcutCaptured")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "settings.shortcutSave" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_sync_shortcut", {
        shortcut: "Control+Shift+Slash",
      });
    });
    expect(screen.getByText("/")).toBeInTheDocument();
  });

  it("keeps the recorder open when a global shortcut is already occupied", async () => {
    installInvokeMock(false);
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_settings") return Promise.resolve(settings);
      if (command === "get_storage_status") return Promise.resolve({
        root: "C:\\TailSync",
        used_bytes: 0,
        quota_bytes: settings.storage_quota_bytes,
        available: true,
      });
      if (command === "get_update_status") return Promise.resolve({
        current_version: "2.1.0",
        updates_enabled: false,
      });
      if (command === "get_peers") return Promise.resolve(peers);
      if (command === "set_sync_shortcut") return Promise.reject(new Error("already registered"));
      return Promise.resolve(undefined);
    });
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.shortcutRecord" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("suspend_sync_shortcut"));
    fireEvent.keyDown(screen.getByRole("button", { name: "settings.shortcutRecording" }), {
      code: "F12",
      key: "F12",
      altKey: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.shortcutSave" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("settings.shortcutConflict");
    expect(screen.getByRole("dialog", { name: "settings.shortcutDialogTitle" })).toBeInTheDocument();
  });
});
