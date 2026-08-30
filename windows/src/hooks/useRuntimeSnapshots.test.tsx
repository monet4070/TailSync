import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useRuntimeSnapshots } from "./useRuntimeSnapshots";

const { waitRuntimeSnapshotMock } = vi.hoisted(() => ({
  waitRuntimeSnapshotMock: vi.fn(),
}));

vi.mock("../tailsyncClient", () => ({
  waitRuntimeSnapshot: waitRuntimeSnapshotMock,
}));

describe("useRuntimeSnapshots", () => {
  beforeEach(() => {
    waitRuntimeSnapshotMock.mockReset();
  });

  it("continues from the last revision and serializes snapshot handling", async () => {
    let resolveSecond: (() => void) | undefined;
    waitRuntimeSnapshotMock
      .mockResolvedValueOnce({
        revision: 7,
        history_version: 3,
        progress: null,
        sync_warning: null,
        notifications: [],
      })
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveSecond = () => resolve({
          revision: 8,
          history_version: 3,
          progress: null,
          sync_warning: null,
          notifications: [],
        });
      }));
    const onSnapshot = vi.fn(async () => undefined);

    const { unmount } = renderHook(() => useRuntimeSnapshots(onSnapshot, 2500));
    await waitFor(() => expect(onSnapshot).toHaveBeenCalledOnce());
    await waitFor(() => expect(waitRuntimeSnapshotMock).toHaveBeenCalledTimes(2));

    expect(waitRuntimeSnapshotMock.mock.calls).toEqual([
      [0, 2500, 0],
      [7, 2500, 0],
    ]);
    unmount();
    resolveSecond?.();
  });
});
