import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDevices } from "./useDevices";
import type { PeersResponse } from "../tailsyncClient";

const { listenMock, eventHandlers } = vi.hoisted(() => ({
  listenMock: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    eventHandlers.set(event, handler);
    return Promise.resolve(vi.fn());
  }),
  eventHandlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

vi.mock("../tailsyncClient", () => ({
  getPeers: vi.fn(),
  refreshPeers: vi.fn(),
  togglePeer: vi.fn(),
}));

import { getPeers, refreshPeers, togglePeer } from "../tailsyncClient";

const mockedGetPeers = vi.mocked(getPeers);
const mockedRefreshPeers = vi.mocked(refreshPeers);
const mockedTogglePeer = vi.mocked(togglePeer);

function makePeers(overrides: Partial<PeersResponse> = {}): PeersResponse {
  return {
    self: {
      hostname: "MacBook",
      tailscale_ip: "100.64.0.5",
      connection_mode: "auto",
      public_key: "pk",
      fingerprint: "fp",
    },
    peers: [],
    paired_peer_endpoints: {},
    discovery_error: null,
    ...overrides,
  };
}

function flushMicrotasks() {
  return act(async () => {});
}

function renderDevices(
  mode: "auto" | "lan_only" | "tailscale_only" | undefined,
  applyPeerEnabled: (hostname: string, enabled: boolean) => void = () => undefined,
) {
  return renderHook(() => useDevices({ connectionMode: mode, applyPeerEnabled }));
}

describe("useDevices", () => {
  beforeEach(() => {
    eventHandlers.clear();
    listenMock.mockClear();
    mockedGetPeers.mockReset().mockResolvedValue(makePeers());
    mockedRefreshPeers.mockReset().mockResolvedValue(makePeers());
    mockedTogglePeer.mockReset().mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("loads devices once the connection mode is known", async () => {
    mockedGetPeers.mockResolvedValue(makePeers({ discovery_error: "no network" }));
    const { result } = renderDevices("auto");
    await flushMicrotasks();

    expect(mockedGetPeers).toHaveBeenCalledTimes(1);
    expect(result.current.devices?.self.hostname).toBe("MacBook");
    expect(result.current.devicesError).toBe("no network");
    expect(result.current.devicesLoading).toBe(false);
  });

  it("does nothing before the connection mode is known", async () => {
    const { result } = renderDevices(undefined);
    await flushMicrotasks();

    expect(mockedGetPeers).not.toHaveBeenCalled();
    expect(result.current.devices).toBeNull();
  });

  it("reloads on peer-health-changed events", async () => {
    const { result } = renderDevices("auto");
    await flushMicrotasks();
    const handler = eventHandlers.get("peer-health-changed");
    expect(handler).toBeDefined();

    mockedGetPeers.mockResolvedValue(makePeers({ peers: [] }));
    await act(async () => {
      handler!({ payload: {} });
      await Promise.resolve();
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(2);
    expect(result.current.devices).not.toBeNull();
  });

  it("polls every 5 seconds while the document is visible", async () => {
    vi.useFakeTimers();
    const { result } = renderDevices("auto");
    await flushMicrotasks();
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(4);
    expect(result.current.devices).not.toBeNull();
  });

  it("skips polling while the document is hidden", async () => {
    vi.useFakeTimers();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    renderDevices("auto");
    await flushMicrotasks();
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(15000);
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);
  });

  it("stops polling after unmount", async () => {
    vi.useFakeTimers();
    const { unmount } = renderDevices("auto");
    await flushMicrotasks();
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);

    await act(async () => {
      await unmount();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15000);
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);
  });

  it("refreshDevices uses refreshPeers and tracks the loading flag", async () => {
    let resolveRefresh!: (peers: PeersResponse) => void;
    mockedRefreshPeers.mockReturnValue(new Promise((resolve) => {
      resolveRefresh = resolve;
    }));
    const { result } = renderDevices("auto");
    await flushMicrotasks();

    let pending!: Promise<void>;
    act(() => {
      pending = result.current.refreshDevices();
    });
    await flushMicrotasks();
    expect(result.current.devicesLoading).toBe(true);

    await act(async () => {
      resolveRefresh(makePeers());
      await pending;
    });
    expect(mockedRefreshPeers).toHaveBeenCalledTimes(1);
    expect(result.current.devicesLoading).toBe(false);
    expect(result.current.devices).not.toBeNull();
  });

  it("refreshDevices failure clears the snapshot and sets the error", async () => {
    mockedRefreshPeers.mockRejectedValue(new Error("offline"));
    const { result } = renderDevices("auto");
    await flushMicrotasks();

    await act(async () => {
      await result.current.refreshDevices();
    });
    expect(result.current.devices).toBeNull();
    expect(result.current.devicesError).toBe("Error: offline");
    expect(result.current.devicesLoading).toBe(false);
  });

  it("resetDevices clears the snapshot and the error", async () => {
    mockedGetPeers.mockResolvedValue(makePeers({ discovery_error: "no network" }));
    const { result } = renderDevices("auto");
    await flushMicrotasks();
    expect(result.current.devicesError).toBe("no network");

    act(() => {
      result.current.resetDevices();
    });
    expect(result.current.devices).toBeNull();
    expect(result.current.devicesError).toBe("");
  });

  it("onDevicesRefreshed reloads silently with getPeers", async () => {
    mockedGetPeers.mockResolvedValue(makePeers({ discovery_error: "stale" }));
    const { result } = renderDevices("auto");
    await flushMicrotasks();

    mockedGetPeers.mockResolvedValue(makePeers());
    await act(async () => {
      await result.current.onDevicesRefreshed();
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(2);
    expect(mockedRefreshPeers).not.toHaveBeenCalled();
    expect(result.current.devicesError).toBe("");
  });

  it("handlePeerToggle optimistically updates, toggles, and patches the hub", async () => {
    mockedGetPeers.mockResolvedValue(makePeers({
      peers: [{
        hostname: "laptop",
        tailscale_ip: "100.64.0.6",
        address: "192.168.1.6",
        online: true,
        enabled: false,
        connection_mode: "auto",
        trusted: false,
        fingerprint: "fp2",
      }],
    }));
    const applyPeerEnabled = vi.fn();
    const { result } = renderDevices("auto", applyPeerEnabled);
    await flushMicrotasks();

    await act(async () => {
      await result.current.handlePeerToggle(
        { hostname: "laptop", enabled: true } as never,
        true,
      );
    });
    expect(mockedTogglePeer).toHaveBeenCalledWith("laptop", true);
    expect(applyPeerEnabled).toHaveBeenCalledWith("laptop", true);
    expect(result.current.devices?.peers[0]?.enabled).toBe(true);
  });

  it("handlePeerToggle refreshes the snapshot when toggling fails", async () => {
    mockedTogglePeer.mockRejectedValue(new Error("denied"));
    const applyPeerEnabled = vi.fn();
    const { result } = renderDevices("auto", applyPeerEnabled);
    await flushMicrotasks();

    await act(async () => {
      await result.current.handlePeerToggle(
        { hostname: "laptop", enabled: true } as never,
        true,
      );
    });
    expect(applyPeerEnabled).not.toHaveBeenCalled();
    expect(mockedRefreshPeers).toHaveBeenCalledTimes(1);
  });

  it("re-arms polling when the connection mode changes", async () => {
    vi.useFakeTimers();
    const applyPeerEnabled = vi.fn();
    const { rerender } = renderHook(
      ({ mode }: { mode: "auto" | "lan_only" | "tailscale_only" | undefined }) =>
        useDevices({ connectionMode: mode, applyPeerEnabled }),
      { initialProps: { mode: "auto" } },
    );
    await flushMicrotasks();
    expect(mockedGetPeers).toHaveBeenCalledTimes(1);

    await act(async () => {
      rerender({ mode: "lan_only" });
      await flushMicrotasks();
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(mockedGetPeers).toHaveBeenCalledTimes(3);
  });
});
