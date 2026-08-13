import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../types/settings.generated";
import { Settings } from "./Settings";

const {
  hideMock,
  invokeMock,
  listenMock,
  openMock,
  setColorThemeMock,
  setLocaleMock,
  setThemeMock,
} = vi.hoisted(() => ({
  hideMock: vi.fn(),
  invokeMock: vi.fn(),
  listenMock: vi.fn(() => Promise.resolve(vi.fn())),
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
});
