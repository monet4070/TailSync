import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConnectionTests } from "./useConnectionTests";

vi.mock("../tailsyncClient", () => ({
  testConnection: vi.fn(),
}));

import { testConnection } from "../tailsyncClient";

const mockedTestConnection = vi.mocked(testConnection);

const peer = {
  hostname: "mac",
  tailscale_ip: "",
  address: "",
  online: true,
  enabled: true,
  connection_mode: "auto" as const,
  trusted: true,
  fingerprint: "",
};

const route = {
  interface: "lan" as const,
  address: "192.168.1.5",
  status: "online" as const,
  online: true,
  connected: false,
};

const KEY = "mac|lan|192.168.1.5";

describe("useConnectionTests", () => {
  beforeEach(() => {
    mockedTestConnection.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  it("tracks testing then success and refreshes devices", async () => {
    mockedTestConnection.mockResolvedValue({ latency_ms: 12, path: "tcp" });
    const onDevicesRefreshed = vi.fn(async () => undefined);
    const { result } = renderHook(() => useConnectionTests(onDevicesRefreshed));

    let promise: Promise<void>;
    act(() => {
      promise = result.current.handleTestConnection(peer, route);
    });
    expect(result.current.connectionTests[KEY]).toEqual({ status: "testing" });

    await act(async () => {
      await promise!;
    });
    expect(result.current.connectionTests[KEY]).toEqual({
      status: "success",
      latency_ms: 12,
      path: "tcp",
    });
    expect(onDevicesRefreshed).toHaveBeenCalledTimes(1);
  });

  it("marks failure as error and still refreshes", async () => {
    mockedTestConnection.mockRejectedValue(new Error("timeout"));
    const onDevicesRefreshed = vi.fn(async () => undefined);
    const { result } = renderHook(() => useConnectionTests(onDevicesRefreshed));

    await act(async () => {
      await result.current.handleTestConnection(peer, route);
    });
    expect(result.current.connectionTests[KEY]).toEqual({ status: "error" });
    expect(onDevicesRefreshed).toHaveBeenCalledTimes(1);
  });

  it("keeps the test result visible when the refresh fails", async () => {
    mockedTestConnection.mockResolvedValue({ latency_ms: 5 });
    const onDevicesRefreshed = vi.fn(async () => {
      throw new Error("refresh failed");
    });
    const { result } = renderHook(() => useConnectionTests(onDevicesRefreshed));

    await act(async () => {
      await result.current.handleTestConnection(peer, route);
    });
    expect(result.current.connectionTests[KEY].status).toBe("success");
  });
});
