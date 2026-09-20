import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  useLaunchAtLogin,
  type LaunchAtLoginService,
} from "./useLaunchAtLogin";

describe("useLaunchAtLogin", () => {
  it("hydrates from the Windows startup registry instead of app settings", async () => {
    const service: LaunchAtLoginService = {
      isEnabled: vi.fn().mockResolvedValue(true),
      enable: vi.fn(),
      disable: vi.fn(),
    };

    const { result } = renderHook(() => useLaunchAtLogin(service));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.enabled).toBe(true);
    expect(service.isEnabled).toHaveBeenCalledOnce();
  });

  it("changes the system registration and verifies the canonical state", async () => {
    const isEnabled = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const service: LaunchAtLoginService = {
      isEnabled,
      enable: vi.fn().mockResolvedValue(undefined),
      disable: vi.fn(),
    };

    const { result } = renderHook(() => useLaunchAtLogin(service));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      expect(await result.current.setEnabled(true)).toBe(true);
    });

    expect(service.enable).toHaveBeenCalledOnce();
    expect(result.current.enabled).toBe(true);
    expect(result.current.error).toBe("");
  });

  it("restores the system truth and exposes an actionable error on failure", async () => {
    const isEnabled = vi.fn().mockResolvedValue(false);
    const service: LaunchAtLoginService = {
      isEnabled,
      enable: vi.fn().mockRejectedValue(new Error("registry access denied")),
      disable: vi.fn(),
    };

    const { result } = renderHook(() => useLaunchAtLogin(service));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      expect(await result.current.setEnabled(true)).toBe(false);
    });

    expect(result.current.enabled).toBe(false);
    expect(result.current.error).toBe("registry access denied");
  });
});
