import { afterEach, describe, expect, it, vi } from "vitest";
import { getLocalIpcMetrics, invoke, LocalIpcMetrics } from "./localIpcMetrics";

const { tauriInvokeMock } = vi.hoisted(() => ({ tauriInvokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvokeMock }));

afterEach(() => {
  vi.unstubAllEnvs();
  tauriInvokeMock.mockReset();
});

describe("local IPC diagnostic metrics", () => {
  it("reports only bounded command counts and timing percentiles", () => {
    const metrics = new LocalIpcMetrics();
    for (let index = 0; index < 300; index += 1) {
      metrics.record("get_history_page", index * 10, index * 10 + index, index !== 299);
    }
    const snapshot = metrics.snapshot(3000);
    expect(snapshot.schema).toBe(1);
    expect(snapshot.elapsed_ms).toBe(3000);
    expect(snapshot.commands).toEqual([{
      command: "get_history_page",
      count: 300,
      failures: 1,
      calls_per_second: 100,
      sampled_calls: 256,
      p50_ms: 171,
      p95_ms: 287,
    }]);
    expect(JSON.stringify(snapshot)).not.toContain("clipboard");
    expect(JSON.stringify(snapshot)).not.toContain("path");
  });

  it("keeps an empty diagnostic session empty", () => {
    expect(new LocalIpcMetrics().snapshot(42)).toEqual({
      schema: 1,
      elapsed_ms: 0,
      commands: [],
    });
  });

  it("is off by default and never records arguments or results when enabled", async () => {
    tauriInvokeMock.mockResolvedValueOnce({ privateValue: "secret-result" });
    vi.stubEnv("VITE_TAILSYNC_DIAGNOSTICS", "0");
    await invoke("get_history_page", { privateValue: "secret-argument" });
    expect(getLocalIpcMetrics()).toBeNull();

    tauriInvokeMock.mockResolvedValueOnce(undefined);
    await invoke("close_history_window");
    expect(tauriInvokeMock).toHaveBeenLastCalledWith("close_history_window");

    tauriInvokeMock.mockResolvedValueOnce({ privateValue: "secret-result" });
    vi.stubEnv("VITE_TAILSYNC_DIAGNOSTICS", "1");
    await invoke("get_history_page", { privateValue: "secret-argument" });
    const encoded = JSON.stringify(getLocalIpcMetrics());
    expect(encoded).toContain("get_history_page");
    expect(encoded).not.toContain("secret-argument");
    expect(encoded).not.toContain("secret-result");
  });
});
