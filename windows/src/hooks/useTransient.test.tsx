import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTransient } from "./useTransient";

describe("useTransient", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("flashes a value and auto-clears after the duration", () => {
    const { result } = renderHook(() => useTransient<string>("", 1500));
    act(() => result.current[1]("hello"));
    expect(result.current[0]).toBe("hello");
    act(() => vi.advanceTimersByTime(1499));
    expect(result.current[0]).toBe("hello");
    act(() => vi.advanceTimersByTime(1));
    expect(result.current[0]).toBe("");
  });

  it("restarts the timer on repeated flashes", () => {
    const { result } = renderHook(() => useTransient<string>("", 1500));
    act(() => result.current[1]("one"));
    act(() => vi.advanceTimersByTime(1000));
    act(() => result.current[1]("two"));
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current[0]).toBe("two");
    act(() => vi.advanceTimersByTime(500));
    expect(result.current[0]).toBe("");
  });

  it("clears immediately and cancels the pending timer", () => {
    const { result } = renderHook(() => useTransient<string>("", 1500));
    act(() => result.current[1]("hello"));
    act(() => result.current[2]());
    expect(result.current[0]).toBe("");
    act(() => vi.advanceTimersByTime(2000));
    expect(result.current[0]).toBe("");
  });
});
