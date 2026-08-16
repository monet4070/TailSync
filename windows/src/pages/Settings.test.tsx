import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../types/settings.generated";
import type { PeerDevice, PeerRoute } from "../tailsyncClient";
import { pairingAddressForPeer } from "../utils/pairingAddress";
import { routeSupportsLatencyTest, Settings } from "./Settings";

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
  history_shortcut: "CommandOrControl+Shift+H",
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

  it("records and persists the independent history-window shortcut", async () => {
    installInvokeMock(false);
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.historyShortcutRecord" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("suspend_sync_shortcut"));
    fireEvent.keyDown(screen.getByRole("button", { name: "settings.shortcutRecording" }), {
      code: "KeyJ",
      key: "j",
      ctrlKey: true,
      shiftKey: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.shortcutSave" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_history_shortcut", {
        shortcut: "Control+Shift+KeyJ",
      });
    });
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

describe("pairingAddressForPeer", () => {
  const peer = (overrides: Partial<PeerDevice> = {}): PeerDevice => ({
    hostname: "MacBook",
    tailscale_ip: "100.64.0.5",
    address: "192.168.1.10",
    online: true,
    enabled: true,
    connection_mode: "auto",
    trusted: false,
    fingerprint: "abcd",
    ...overrides,
  });

  it("prefers the iroh route over other candidates", () => {
    const result = pairingAddressForPeer(peer({
      routes: [
        { interface: "lan", address: "192.168.1.10", status: "online", online: true, connected: false, latency_ms: 2 },
        { interface: "iroh", address: "5866666666666666666666666666666666666666666666666666666666666666", status: "online", online: true, connected: false, latency_ms: 30 },
      ],
    }));
    expect(result).toBe("5866666666666666666666666666666666666666666666666666666666666666");
  });

  it("returns null when the active route is iroh but no route lists an address", () => {
    expect(pairingAddressForPeer(peer({ current_interface: "iroh", routes: [] }))).toBeNull();
  });

  it("falls back to the connected route and then the peer address for TCP pairing", () => {
    expect(pairingAddressForPeer(peer({
      routes: [
        { interface: "tailscale", address: "100.64.0.5", status: "online", online: true, connected: true, latency_ms: 20 },
      ],
    }))).toBe("100.64.0.5");
    expect(pairingAddressForPeer(peer({ routes: [], address: "", tailscale_ip: "100.64.0.5" }))).toBe("100.64.0.5");
    expect(pairingAddressForPeer(peer({ routes: [], address: "", tailscale_ip: "" }))).toBeNull();
  });
});

describe("routeSupportsLatencyTest", () => {
  const route = (overrides: Partial<PeerRoute> = {}): PeerRoute => ({
    interface: "iroh" as const,
    address: "endpoint",
    status: "online" as const,
    online: true,
    connected: false,
    ...overrides,
  });

  it("fails closed for an iroh route without an explicit capability", () => {
    expect(routeSupportsLatencyTest(route())).toBe(false);
    expect(routeSupportsLatencyTest(route({ rtt_capable: false }))).toBe(false);
    expect(routeSupportsLatencyTest(route({ rtt_capable: true }))).toBe(true);
  });

  it("keeps TCP route tests available", () => {
    expect(routeSupportsLatencyTest(route({ interface: "lan" }))).toBe(true);
    expect(routeSupportsLatencyTest(route({ interface: "tailscale" }))).toBe(true);
  });
});
