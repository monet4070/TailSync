import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../tailsyncClient", () => ({
  cancelRemotePairingInvite: vi.fn(),
  createRemotePairingInvite: vi.fn(),
  inspectRemotePairingLink: vi.fn(),
  startRemotePairing: vi.fn(),
  takePendingRemotePairingLink: vi.fn(),
}));

import { listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";
import {
  cancelRemotePairingInvite,
  createRemotePairingInvite,
  inspectRemotePairingLink,
  startRemotePairing,
  takePendingRemotePairingLink,
} from "../tailsyncClient";
import { useRemotePairing } from "./useRemotePairing";

const LINK = "tailsync://pair/v1/test-invite";
const PREVIEW = {
  endpoint_id: "endpoint-a",
  expires_at: 1_900_000_000,
  remaining_seconds: 120,
};

const mockedListen = vi.mocked(listen);
const mockedCancel = vi.mocked(cancelRemotePairingInvite);
const mockedCreate = vi.mocked(createRemotePairingInvite);
const mockedInspect = vi.mocked(inspectRemotePairingLink);
const mockedStart = vi.mocked(startRemotePairing);
const mockedTakePending = vi.mocked(takePendingRemotePairingLink);

let remoteLinkListener: ((event: Event<unknown>) => void) | undefined;
let unlisten: Mock<() => void>;

async function flushMicrotasks() {
  await act(async () => {});
}

describe("useRemotePairing", () => {
  beforeEach(() => {
    remoteLinkListener = undefined;
    unlisten = vi.fn<() => void>();
    mockedListen.mockReset().mockImplementation(async (_event, handler) => {
      remoteLinkListener = handler;
      const stop: UnlistenFn = () => { unlisten(); };
      return stop;
    });
    mockedCancel.mockReset().mockResolvedValue({
      pairing_enabled: false,
      phase: "cancelled",
      remaining_seconds: 0,
      failed_attempts: 0,
      max_failures: 5,
    });
    mockedCreate.mockReset().mockResolvedValue({
      link: LINK,
      expires_at: PREVIEW.expires_at,
      remaining_seconds: PREVIEW.remaining_seconds,
    });
    mockedInspect.mockReset().mockResolvedValue(PREVIEW);
    mockedStart.mockReset().mockResolvedValue({
      pairing_enabled: true,
      phase: "verification",
      remaining_seconds: 100,
      failed_attempts: 0,
      max_failures: 5,
    });
    mockedTakePending.mockReset().mockResolvedValue(null);
  });

  it("loads and inspects a validated cold-start link from the native inbox", async () => {
    mockedTakePending.mockResolvedValueOnce(LINK);

    const { result } = renderHook(() => useRemotePairing());
    await flushMicrotasks();

    expect(result.current.linkDraft).toBe(LINK);
    expect(mockedInspect).toHaveBeenCalledWith(LINK);
    expect(result.current.linkPreview).toEqual(PREVIEW);
  });

  it("loads a hot-start link when the native event announces new inbox data", async () => {
    mockedTakePending
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(LINK);
    const { result } = renderHook(() => useRemotePairing());
    await flushMicrotasks();

    await act(async () => {
      remoteLinkListener?.({ event: "remote-pairing-link-received", id: 1, payload: null });
    });

    expect(mockedTakePending).toHaveBeenCalledTimes(2);
    expect(result.current.linkDraft).toBe(LINK);
    expect(result.current.linkPreview).toEqual(PREVIEW);
  });

  it("starts pairing with the inspected link and releases its listener on unmount", async () => {
    const { result, unmount } = renderHook(() => useRemotePairing());
    await flushMicrotasks();

    act(() => result.current.handleLinkChange(LINK));
    await act(async () => {
      await result.current.handleInspectLink();
      await result.current.handleStartRemotePairing();
    });

    expect(mockedInspect).toHaveBeenCalledWith(LINK);
    expect(mockedStart).toHaveBeenCalledWith(LINK);
    expect(result.current.remotePairingError).toBe("");
    expect(result.current.remotePairingBusy).toBe(false);

    unmount();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
