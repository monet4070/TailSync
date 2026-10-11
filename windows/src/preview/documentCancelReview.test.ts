import { describe, expect, it, vi } from "vitest";
import { cancelDocumentPreview } from "../tailsyncClient";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../diagnostics/localIpcMetrics", () => ({ invoke: bridge.invoke }));

describe("document cancellation admission", () => {
  it("keeps one running cancellation and at most the newest request per owner", async () => {
    let finish!: () => void;
    bridge.invoke.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
    const request = { entry_id: 7, owner: "history" as const, generation: 1, request_id: "cancel-1" };
    const running = cancelDocumentPreview(request);
    for (let generation = 2; generation < 40; generation += 1) {
      void cancelDocumentPreview({ ...request, generation, request_id: `cancel-${generation}` });
      void cancelDocumentPreview({ ...request, owner: "favorites", generation, request_id: `favorite-${generation}` });
    }
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    finish();
    await running;
    expect(bridge.invoke).toHaveBeenCalledTimes(3);
    expect(bridge.invoke.mock.calls.slice(1).map(([, value]) => value.request.generation)).toEqual([39, 39]);
  });
});
