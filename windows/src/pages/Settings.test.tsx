import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../types/settings.generated";
import type { PeerDevice, PeerRoute } from "../tailsyncClient";
import { pairingAddressForPeer } from "../utils/pairingAddress";
import { Settings } from "./Settings";
import { routeSupportsLatencyTest } from "../utils/peerRoute";

const {
  hideMock,
  invokeMock,
  listenMock,
  eventHandlers,
  openMock,
  setColorThemeMock,
  setLocaleMock,
  setThemeMock,
  refreshCustomThemesMock,
  useThemeState,
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
  refreshCustomThemesMock: vi.fn(),
  useThemeState: {
    customThemes: [] as import("../tailsyncClient").ThemeEntry[],
    themeLoadErrors: [] as import("../tailsyncClient").ThemeErrorItem[],
    colorTheme: "tailsync",
  },
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
  customThemeId: (value: string) =>
    value.startsWith("custom:") ? value.slice("custom:".length) : null,
  useTheme: () => ({
    theme: "light",
    themePreference: "system",
    colorTheme: useThemeState.colorTheme,
    setTheme: setThemeMock,
    setColorTheme: setColorThemeMock,
    resolvedColorTheme: "tailsync",
    customThemes: useThemeState.customThemes,
    themeLoadErrors: useThemeState.themeLoadErrors,
    refreshCustomThemes: refreshCustomThemesMock,
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

function installInvokeMock(
  updatesEnabled: boolean,
  overrides: Record<string, (args?: unknown) => unknown> = {},
) {
  invokeMock.mockImplementation((command: string, args?: unknown) => {
    const override = overrides[command];
    if (override) return override(args);
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

describe("Custom themes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventHandlers.clear();
    installInvokeMock(false);
    useThemeState.customThemes = [];
    useThemeState.themeLoadErrors = [];
    useThemeState.colorTheme = "tailsync";
    openMock.mockResolvedValue(null);
  });

  function paletteSpec(hex: string) {
    return { hex };
  }

  const studioEntry: import("../tailsyncClient").ThemeEntry = {
    id: "studio",
    name: { en: "Studio", "zh-CN": "工作室" },
    file: "studio.json",
    palette: {
      light: {
        brand: paletteSpec("#d5684b"),
        brandHover: paletteSpec("#bb553b"),
        brandSoft: paletteSpec("#d5684b"),
        brandText: paletteSpec("#ffffff"),
        bgWindow: paletteSpec("#faf9f5"),
        bgCard: paletteSpec("#fffefa"),
        bgInput: paletteSpec("#1a1916"),
        bgHover: paletteSpec("#f0eee7"),
        bgActive: paletteSpec("#e8e4da"),
        bgRaised: paletteSpec("#fffefa"),
        bgToast: paletteSpec("#171716"),
        textPrimary: paletteSpec("#191918"),
        textSecondary: paletteSpec("#68665f"),
        textTertiary: paletteSpec("#98958b"),
        textToast: paletteSpec("#ffffff"),
        border: paletteSpec("#e7e3d9"),
        borderStrong: paletteSpec("#d3cec2"),
        divider: paletteSpec("#ece8df"),
        green: paletteSpec("#44745a"),
        greenSoft: paletteSpec("#44745a"),
        orange: paletteSpec("#b96536"),
        orangeSoft: paletteSpec("#b96536"),
        purple: paletteSpec("#765b8f"),
        purpleSoft: paletteSpec("#765b8f"),
      },
      dark: {
        brand: paletteSpec("#ec8668"),
        brandHover: paletteSpec("#f29b80"),
        brandSoft: paletteSpec("#ec8668"),
        brandText: paletteSpec("#181412"),
        bgWindow: paletteSpec("#191918"),
        bgCard: paletteSpec("#232321"),
        bgInput: paletteSpec("#fffdf5"),
        bgHover: paletteSpec("#292825"),
        bgActive: paletteSpec("#33312d"),
        bgRaised: paletteSpec("#262522"),
        bgToast: paletteSpec("#f8f5ed"),
        textPrimary: paletteSpec("#f4f1e9"),
        textSecondary: paletteSpec("#aaa69c"),
        textTertiary: paletteSpec("#77746c"),
        textToast: paletteSpec("#171716"),
        border: paletteSpec("#32312d"),
        borderStrong: paletteSpec("#48463f"),
        divider: paletteSpec("#2c2b28"),
        green: paletteSpec("#75aa86"),
        greenSoft: paletteSpec("#75aa86"),
        orange: paletteSpec("#dc9163"),
        orangeSoft: paletteSpec("#dc9163"),
        purple: paletteSpec("#ad8cc6"),
        purpleSoft: paletteSpec("#ad8cc6"),
      },
    },
    metrics: { cardRadius: 10, controlRadius: 9, rowPadding: 13, shadowRadius: 8 },
    typography: {
      sectionTitleSize: 25,
      uppercasesSectionTitles: false,
      searchSize: 18,
      searchUsesDisplayFont: true,
      historyContentSize: 15,
    },
    fonts: { display: "Songti SC", reading: null },
    structural: null,
  };

  it("renders built-in, custom, and invalid cards together", async () => {
    useThemeState.customThemes = [studioEntry];
    useThemeState.themeLoadErrors = [
      { file: "broken.json", reason: "Invalid theme JSON" },
    ];
    render(<Settings />);

    // Built-in cards (existing behaviour)…
    expect(await screen.findByText("settings.colorTheme.tailsync")).toBeInTheDocument();
    expect(screen.getByText("settings.colorTheme.high-contrast")).toBeInTheDocument();
    // …custom card with its localised display name (preview title + label)…
    expect(screen.getAllByText("Studio").length).toBeGreaterThan(0);
    // …and a gray invalid card titled by the file name.
    expect(screen.getByText("broken.json")).toBeInTheDocument();
    expect(screen.getByText("settings.customThemeInvalid")).toBeInTheDocument();
    expect(screen.getByTitle("Invalid theme JSON")).toBeInTheDocument();
    // The group actions are visible.
    expect(screen.getByText("settings.customThemeImport")).toBeInTheDocument();
    expect(screen.getByText("settings.customThemeOpenFolder")).toBeInTheDocument();
  });

  it("warns when the selected custom theme is missing from the catalogue", async () => {
    // R003: a stored `custom:{id}` whose file is absent must surface a
    // localized warning naming the id; the value itself is not rewritten
    // (the daemon keeps it; resolveColorTheme applies the default instead).
    useThemeState.colorTheme = "custom:ghost";
    useThemeState.customThemes = [studioEntry];
    render(<Settings />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("settings.colorThemeMissing");
    expect(alert).toHaveTextContent("ghost");
    // Loading settings seeds React state with the stored value (an
    // in-memory setColorTheme call), but nothing may write the sanitised
    // value back to storage.
    await waitFor(() => {
      expect(invokeMock).not.toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({
          settingsJson: expect.stringContaining("color_theme"),
        }),
      );
    });
  });

  it("does not warn when the selected custom theme exists", async () => {
    useThemeState.colorTheme = "custom:studio";
    useThemeState.customThemes = [studioEntry];
    render(<Settings />);

    expect((await screen.findAllByText("Studio")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does not warn for built-in selections", async () => {
    useThemeState.colorTheme = "ocean";
    render(<Settings />);

    await screen.findByText("settings.colorTheme.ocean");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("clears the warning once the missing theme is imported", async () => {
    // The catalogue refreshes after an import; a now-present id must no
    // longer be flagged on the next render.
    useThemeState.colorTheme = "custom:studio";
    useThemeState.customThemes = [];
    const { rerender } = render(<Settings />);

    expect(await screen.findByRole("alert")).toHaveTextContent("studio");
    useThemeState.customThemes = [studioEntry];
    rerender(<Settings />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("selects a custom theme card", async () => {
    useThemeState.customThemes = [studioEntry];
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "Studio" }));

    expect(setColorThemeMock).toHaveBeenCalledWith("custom:studio");
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({ settingsJson: expect.stringContaining("custom:studio") }),
      );
    });
  });

  it("imports a theme file from the picker and refreshes the catalogue", async () => {
    openMock.mockResolvedValue("/tmp/studio.json");
    installInvokeMock(false, {
      import_theme: () => Promise.resolve(studioEntry),
    });
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.customThemeImport" }));

    expect(openMock).toHaveBeenCalledWith(
      expect.objectContaining({ filters: [{ name: "settings.customThemeImportFilter", extensions: ["json"] }] }),
    );
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("import_theme", { path: "/tmp/studio.json" });
      expect(refreshCustomThemesMock).toHaveBeenCalled();
      expect(screen.getByText("settings.saved")).toBeInTheDocument();
    });
  });

  it("shows the daemon reason when an import fails", async () => {
    openMock.mockResolvedValue("/tmp/broken.json");
    installInvokeMock(false, {
      import_theme: () =>
        Promise.reject('A theme with id "studio" already exists (studio.json)'),
    });
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.customThemeImport" }));

    await waitFor(() => {
      expect(
        screen.getByText('A theme with id "studio" already exists (studio.json)'),
      ).toBeInTheDocument();
    });
    expect(refreshCustomThemesMock).not.toHaveBeenCalled();
  });

  it("deletes a custom theme after confirmation and refreshes", async () => {
    useThemeState.customThemes = [studioEntry];
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.customThemeDelete Studio" }));

    expect(confirmSpy).toHaveBeenCalledWith('settings.customThemeDelete "Studio"?');
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("delete_theme", { themeId: "studio" });
      expect(refreshCustomThemesMock).toHaveBeenCalled();
    });
    confirmSpy.mockRestore();
  });

  it("does not delete when the confirmation is declined", async () => {
    useThemeState.customThemes = [studioEntry];
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.customThemeDelete Studio" }));

    expect(invokeMock).not.toHaveBeenCalledWith("delete_theme", expect.anything());
    expect(refreshCustomThemesMock).not.toHaveBeenCalled();
    vi.restoreAllMocks();
  });

  it("reveals the themes folder", async () => {
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "settings.customThemeOpenFolder" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("reveal_themes_dir");
    });
  });

  it("marks cards of themes with a background (badge + scrim strip)", async () => {
    useThemeState.customThemes = [
      {
        ...studioEntry,
        background: {
          light: {
            hasImage: true,
            scrim: { hex: "#0f1526", opacity: 0.82 },
            mimeType: "image/png",
          },
          dark: { hasImage: false },
        },
      },
    ];
    render(<Settings />);

    expect(await screen.findByText("settings.customThemeHasBackground")).toBeInTheDocument();
    const strip = document.querySelector<HTMLElement>(".custom-theme-bg-strip");
    expect(strip).not.toBeNull();
    expect(strip!.style.backgroundColor).toBe("rgba(15, 21, 38, 0.82)");
    // The indicator is metadata-only: no image fetch happens on the page.
    expect(invokeMock).not.toHaveBeenCalledWith("get_theme_background", expect.anything());
  });

  it("shows the indicator when only one mode has a background", async () => {
    useThemeState.customThemes = [
      {
        ...studioEntry,
        background: {
          light: { hasImage: false },
          dark: {
            hasImage: true,
            scrim: { hex: "#101820", opacity: 0.9 },
            mimeType: "image/jpeg",
          },
        },
      },
    ];
    render(<Settings />);

    expect(await screen.findByText("settings.customThemeHasBackground")).toBeInTheDocument();
    const strip = document.querySelector<HTMLElement>(".custom-theme-bg-strip");
    expect(strip!.style.backgroundColor).toBe("rgba(16, 24, 32, 0.9)");
  });

  it("shows no background indicator for themes without one", async () => {
    useThemeState.customThemes = [{ ...studioEntry, background: null }];
    render(<Settings />);

    await screen.findAllByText("Studio");
    expect(screen.queryByText("settings.customThemeHasBackground")).toBeNull();
    expect(document.querySelector(".custom-theme-bg-strip")).toBeNull();
  });
});
