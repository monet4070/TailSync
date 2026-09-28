import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { Connections } from "./Connections";
import * as client from "../tailsyncClient";
import type { SettingsData } from "../types/settings.generated";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../tailsyncClient", async (importOriginal) => {
  const actual = await importOriginal<typeof client>();
  return {
    ...actual,
    getSettings: vi.fn(),
    updateSettings: vi.fn(),
    getPeers: vi.fn(),
    refreshPeers: vi.fn(),
    closeConnectionsWindow: vi.fn(),
    forgetPeer: vi.fn(),
    getLocalThemeSettingsV2: vi.fn().mockResolvedValue({
      activeThemeId: "builtin:canvas@1",
      appearance: "system",
      highContrast: false,
    }),
    resolveThemeV2: vi.fn().mockResolvedValue({
      tokens: {
        colors: {
          background: { canvas: "#ffffff", surface: "#f5f5f5" },
          text: { primary: "#000", secondary: "#666" },
        },
      },
    }),
  };
});

const mockSettings: SettingsData = {
  connection_mode: "auto",
  sync_enabled: true,
  history_limit: 100,
  storage_quota_bytes: 10737418240,
  language: "zh-CN",
  sync_shortcut: "Alt+V",
  history_shortcut: "Alt+H",
  notifications_enabled: true,
  progress_bar_enabled: true,
  storage_root: null,
  paired_peer_endpoints: {},
  trusted_peer_addresses: {},
  trusted_peer_keys: {},
  enabled_peers: {},
};

const mockDevices: client.PeersResponse = {
  self: {
    hostname: "DESKTOP-TEST",
    fingerprint: "SHA256:LOCALFINGERPRINT123",
    connection_mode: "auto",
    public_key: "LOCALKEY123",
    tailscale_ip: "",
    iroh_endpoint_id: "node123",
    routes: [
      {
        interface: "lan",
        address: "192.168.1.10:4433",
        status: "connected",
        online: true,
        connected: true,
        latency_ms: null,
      },
    ],
  },
  peers: [],
  paired_peer_endpoints: {},
  discovery_error: null,
};

describe("Connections Window", () => {
  beforeEach(() => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: () => ({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    });
    vi.clearAllMocks();
    vi.mocked(client.getSettings).mockResolvedValue(mockSettings);
    vi.mocked(client.getPeers).mockResolvedValue(mockDevices);
    vi.mocked(client.refreshPeers).mockResolvedValue(mockDevices);
  });

  it("renders the connections window with title bar and close action", async () => {
    render(<Connections />);

    await waitFor(() => {
      expect(screen.getAllByText("连接与设备").length).toBeGreaterThan(0);
    });

    const closeButton = screen.getByRole("button", { name: "关闭" });
    fireEvent.click(closeButton);

    expect(client.closeConnectionsWindow).toHaveBeenCalledTimes(1);
  });

  it("renders connection mode options and device discovery", async () => {
    render(<Connections />);

    await waitFor(() => {
      expect(screen.getByRole("radiogroup", { name: "连接方式" })).toBeInTheDocument();
    });

    expect(screen.getByRole("radio", { name: "自动" })).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText("DESKTOP-TEST")).toBeInTheDocument();
    });
    expect(screen.getByText("本机")).toBeInTheDocument();
  });
});
