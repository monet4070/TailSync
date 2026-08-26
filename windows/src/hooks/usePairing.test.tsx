import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePairing } from "./usePairing";

vi.mock("../tailsyncClient", () => ({
  cancelPairing: vi.fn(),
  confirmPairing: vi.fn(),
  enablePairing: vi.fn(),
  getPairingStatus: vi.fn(),
  startPairing: vi.fn(),
}));

vi.mock("../utils/pairingAddress", () => ({
  pairingAddressForPeer: vi.fn(),
}));

import {
  cancelPairing,
  confirmPairing,
  enablePairing,
  getPairingStatus,
  startPairing,
  type PairingStatus,
  type PeerDevice,
} from "../tailsyncClient";
import { pairingAddressForPeer } from "../utils/pairingAddress";

const mockedCancel = vi.mocked(cancelPairing);
const mockedConfirm = vi.mocked(confirmPairing);
const mockedEnable = vi.mocked(enablePairing);
const mockedGetStatus = vi.mocked(getPairingStatus);
const mockedStart = vi.mocked(startPairing);
const mockedAddress = vi.mocked(pairingAddressForPeer);

function makePeer(overrides: Partial<PeerDevice> = {}): PeerDevice {
  return {
    hostname: "MacBook",
    tailscale_ip: "100.64.0.5",
    address: "192.168.1.10",
    online: true,
    enabled: true,
    connection_mode: "auto",
    trusted: false,
    fingerprint: "abcd",
    ...overrides,
  };
}

function makeStatus(overrides: Partial<PairingStatus> = {}): PairingStatus {
  return {
    pairing_enabled: true,
    phase: "waiting",
    remaining_seconds: 60,
    failed_attempts: 0,
    max_failures: 3,
    ...overrides,
  };
}

function flushMicrotasks() {
  return act(async () => {});
}

describe("usePairing", () => {
  beforeEach(() => {
    mockedCancel.mockReset().mockResolvedValue(makeStatus({ phase: "cancelled" }));
    mockedConfirm.mockReset().mockResolvedValue(makeStatus({ phase: "paired" }));
    mockedEnable.mockReset().mockResolvedValue(makeStatus({ phase: "waiting" }));
    mockedGetStatus.mockReset().mockResolvedValue(makeStatus());
    mockedStart.mockReset().mockResolvedValue(makeStatus({ phase: "verification" }));
    mockedAddress.mockReset().mockReturnValue("192.168.1.10");
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("opens pairing for a peer, enabling pairing first when it is disabled", async () => {
    mockedGetStatus.mockResolvedValue(makeStatus({ pairing_enabled: false }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.openPairing(makePeer());
    });
    expect(mockedEnable).toHaveBeenCalledTimes(1);
    expect(mockedStart).toHaveBeenCalledWith("192.168.1.10");
    expect(result.current.pairingStatus?.phase).toBe("verification");
    expect(result.current.pairingTarget?.hostname).toBe("MacBook");
    expect(result.current.pairingOpen).toBe(true);
    expect(result.current.pairingBusy).toBe(false);
    expect(result.current.pairingError).toBe("");
  });

  it("skips enablePairing when pairing is already enabled", async () => {
    mockedGetStatus.mockResolvedValue(makeStatus({ pairing_enabled: true }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.openPairing(makePeer());
    });
    expect(mockedEnable).not.toHaveBeenCalled();
    expect(mockedStart).toHaveBeenCalledWith("192.168.1.10");
  });

  it("does nothing when the peer has no pairing address", async () => {
    mockedAddress.mockReturnValue(null);
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.openPairing(makePeer());
    });
    expect(mockedStart).not.toHaveBeenCalled();
    expect(result.current.pairingOpen).toBe(false);
    expect(result.current.pairingTarget).toBeNull();
  });

  it("re-syncs the status when starting the handshake fails", async () => {
    mockedStart.mockRejectedValue(new Error("boom"));
    mockedGetStatus.mockResolvedValue(makeStatus({ phase: "waiting" }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.openPairing(makePeer());
    });
    expect(result.current.pairingError).toBe("Error: boom");
    expect(result.current.pairingStatus?.phase).toBe("waiting");
    expect(result.current.pairingBusy).toBe(false);
  });

  it("enables pairing from the settings row", async () => {
    mockedEnable.mockResolvedValue(makeStatus({ phase: "waiting" }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.handleEnablePairing();
    });
    expect(mockedEnable).toHaveBeenCalledTimes(1);
    expect(result.current.pairingOpen).toBe(true);
    expect(result.current.pairingBusy).toBe(false);
  });

  it("closes pairing and cancels the handshake", async () => {
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.handleEnablePairing();
    });
    await act(async () => {
      await result.current.closePairing();
    });
    expect(mockedCancel).toHaveBeenCalledTimes(1);
    expect(result.current.pairingOpen).toBe(false);
    expect(result.current.pairingTarget).toBeNull();
    expect(result.current.pairingBusy).toBe(false);
  });

  it("ignores close requests while a pairing action is in flight", async () => {
    let resolveCancel!: (status: PairingStatus) => void;
    mockedCancel.mockReturnValue(new Promise((resolve) => {
      resolveCancel = resolve;
    }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      const first = result.current.closePairing();
      const second = result.current.closePairing();
      resolveCancel(makeStatus({ phase: "cancelled" }));
      await first;
      await second;
    });
    expect(mockedCancel).toHaveBeenCalledTimes(1);
    expect(result.current.pairingOpen).toBe(false);
  });

  it("confirms the handshake when a peer is present", async () => {
    mockedGetStatus.mockResolvedValue(makeStatus({
      phase: "verification",
      peer: {
        hostname: "MacBook",
        address: "192.168.1.10",
        fingerprint: "abcd",
        verification_code: "1234",
        local_confirmed: false,
        remote_confirmed: false,
      },
    }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.handlePair();
    });
    expect(mockedConfirm).toHaveBeenCalledTimes(1);
    expect(result.current.pairingStatus?.phase).toBe("paired");
  });

  it("does nothing when confirming without a peer", async () => {
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();

    await act(async () => {
      await result.current.handlePair();
    });
    expect(mockedConfirm).not.toHaveBeenCalled();
  });

  it("polls and auto-opens the dialog during verification", async () => {
    vi.useFakeTimers();
    mockedGetStatus
      .mockResolvedValueOnce(makeStatus({ phase: "waiting" }))
      .mockResolvedValue(makeStatus({
        phase: "verification",
        peer: {
          hostname: "MacBook",
          address: "192.168.1.10",
          fingerprint: "abcd",
          verification_code: "1234",
          local_confirmed: false,
          remote_confirmed: false,
        },
      }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();
    expect(result.current.pairingOpen).toBe(false);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.pairingOpen).toBe(true);
    expect(result.current.pairingStatus?.phase).toBe("verification");
  });

  it("keeps the dialog open while pairing is finalizing", async () => {
    vi.useFakeTimers();
    mockedGetStatus
      .mockResolvedValueOnce(makeStatus({ phase: "waiting" }))
      .mockResolvedValue(makeStatus({
        phase: "finalizing",
        peer: {
          hostname: "MacBook",
          address: "192.168.1.10",
          fingerprint: "abcd",
          verification_code: "1234",
          local_confirmed: true,
          remote_confirmed: true,
        },
      }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();
    expect(result.current.pairingOpen).toBe(false);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.pairingOpen).toBe(true);
    expect(result.current.pairingStatus?.phase).toBe("finalizing");
  });

  it("closes the dialog and refreshes devices once when a pairing completes", async () => {
    vi.useFakeTimers();
    mockedGetStatus
      .mockResolvedValueOnce(makeStatus({
        phase: "verification",
        peer: {
          hostname: "MacBook",
          address: "192.168.1.10",
          fingerprint: "abcd",
          verification_code: "1234",
          local_confirmed: false,
          remote_confirmed: false,
        },
      }))
      .mockResolvedValue(makeStatus({ phase: "paired" }));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();
    expect(result.current.pairingOpen).toBe(true);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.pairingOpen).toBe(false);
    expect(result.current.pairingTarget).toBeNull();
    expect(refreshDevices).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(refreshDevices).toHaveBeenCalledTimes(1);
  });

  it("surfaces polling failures in the dialog error", async () => {
    mockedGetStatus.mockRejectedValue(new Error("offline"));
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();
    expect(result.current.pairingError).toBe("Error: offline");
  });

  it("stops polling after unmount", async () => {
    vi.useFakeTimers();
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { unmount } = renderHook(() => usePairing({ refreshDevices }));
    await flushMicrotasks();
    const callsAfterMount = mockedGetStatus.mock.calls.length;

    await act(async () => {
      await unmount();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(mockedGetStatus.mock.calls.length).toBe(callsAfterMount);
  });
});
