import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHistoryNotice } from "./useHistoryNotice";

describe("useHistoryNotice", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("does not extend a repeated notice deadline", () => {
    const { result } = renderHook(() => useHistoryNotice());
    const warning = {
      key: "sync-warning:peer-a",
      level: "warning" as const,
      message: "Connection is not ready",
    };

    act(() => result.current[1](warning));
    act(() => vi.advanceTimersByTime(2_000));
    act(() => result.current[1](warning));
    expect(result.current[0]?.occurrences).toBe(2);

    act(() => vi.advanceTimersByTime(2_499));
    expect(result.current[0]).not.toBeNull();
    act(() => vi.advanceTimersByTime(1));
    expect(result.current[0]).toBeNull();
  });

  it("replaces a different notice and supports manual dismissal", () => {
    const { result } = renderHook(() => useHistoryNotice());
    act(() => result.current[1]({ key: "one", level: "error", message: "One" }));
    act(() => result.current[1]({ key: "two", level: "success", message: "Two" }));
    expect(result.current[0]).toMatchObject({ key: "two", message: "Two", occurrences: 1 });
    act(() => result.current[2]());
    expect(result.current[0]).toBeNull();
    act(() => vi.advanceTimersByTime(6_000));
    expect(result.current[0]).toBeNull();
  });

  it("forces a visible break when different errors keep replacing each other", () => {
    const { result } = renderHook(() => useHistoryNotice());

    act(() => result.current[1]({ key: "one", level: "error", message: "One" }));
    act(() => vi.advanceTimersByTime(5_000));
    act(() => result.current[1]({ key: "two", level: "error", message: "Two" }));
    act(() => vi.advanceTimersByTime(2_999));
    act(() => result.current[1]({ key: "three", level: "error", message: "Three" }));
    expect(result.current[0]?.key).toBe("three");

    act(() => vi.advanceTimersByTime(1));
    expect(result.current[0]).toBeNull();

    act(() => result.current[1]({ key: "four", level: "error", message: "Four" }));
    expect(result.current[0]).toBeNull();
    act(() => vi.advanceTimersByTime(1_500));
    act(() => result.current[1]({ key: "five", level: "error", message: "Five" }));
    expect(result.current[0]?.key).toBe("five");
  });
});
