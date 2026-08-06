import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useVisiblePolling } from "./useVisiblePolling";

describe("useVisiblePolling", () => {
  let visibilityState: DocumentVisibilityState;

  beforeEach(() => {
    vi.useFakeTimers();
    visibilityState = "visible";
    vi.spyOn(document, "visibilityState", "get").mockImplementation(
      () => visibilityState,
    );
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs serially and pauses while the document is hidden", async () => {
    const task = vi.fn(async () => undefined);
    renderHook(() => useVisiblePolling(task, 100));
    await act(async () => Promise.resolve());
    expect(task).toHaveBeenCalledTimes(1);

    visibilityState = "hidden";
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(task).toHaveBeenCalledTimes(1);

    visibilityState = "visible";
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => Promise.resolve());
    expect(task).toHaveBeenCalledTimes(2);
  });

  it("uses the latest task without restarting the timer", async () => {
    const first = vi.fn(async () => undefined);
    const second = vi.fn(async () => undefined);
    const { rerender } = renderHook(
      ({ task }) => useVisiblePolling(task, 100),
      { initialProps: { task: first } },
    );
    await act(async () => Promise.resolve());
    rerender({ task: second });

    await act(async () => vi.advanceTimersByTimeAsync(100));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });
});
