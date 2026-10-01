import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHistoryNotice } from "./useHistoryNotice";
import { useSyncWarningNotice } from "./useSyncWarningNotice";
import { ackSyncWarning, type SyncWarning } from "../tailsyncClient";

vi.mock("../tailsyncClient", () => ({ ackSyncWarning: vi.fn() }));
const warning: SyncWarning = { id: 7, kind: "expired_event", peer: "peer", occurred_at_ms: 1 };
function Harness() {
  const [notice, show] = useHistoryNotice();
  useSyncWarningNotice(warning, "Expired warning", notice, show);
  return <div>{notice && `${notice.message}:${notice.occurrences}`}</div>;
}

describe("useSyncWarningNotice", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(ackSyncWarning).mockReset().mockResolvedValue(true);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });
  afterEach(() => vi.useRealTimers());

  it("retries an ack failure without redisplaying or extending the notice", async () => {
    vi.mocked(ackSyncWarning).mockRejectedValueOnce(new Error("offline"));
    await act(async () => { render(<Harness />); });
    expect(screen.getByText("Expired warning:1")).toBeInTheDocument();
    expect(ackSyncWarning).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(ackSyncWarning).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Expired warning:1")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(3500); });
    expect(screen.queryByText("Expired warning:1")).not.toBeInTheDocument();
    expect(ackSyncWarning).toHaveBeenCalledTimes(2);
  });

  it("waits for a visible render even without another daemon snapshot", async () => {
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    await act(async () => { render(<Harness />); });
    expect(ackSyncWarning).not.toHaveBeenCalled();
    expect(screen.queryByText("Expired warning:1")).not.toBeInTheDocument();
    await act(async () => {
      Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(screen.getByText("Expired warning:1")).toBeInTheDocument();
    expect(ackSyncWarning).toHaveBeenCalledExactlyOnceWith(7);
  });
});
